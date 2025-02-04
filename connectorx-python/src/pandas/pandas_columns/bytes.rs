use super::{check_dtype, HasPandasColumn, PandasColumn, PandasColumnObject, GIL_MUTEX};
use crate::errors::ConnectorXPythonError;
use anyhow::anyhow;
use fehler::throws;
use ndarray::{ArrayViewMut2, Axis, Ix2};
use numpy::{npyffi::NPY_TYPES, Element, PyArray, PyArrayDescr};
use pyo3::{FromPyObject, Py, PyAny, PyResult, Python};
use std::any::TypeId;

#[derive(Clone)]
#[repr(transparent)]
pub struct PyBytes(Py<pyo3::types::PyBytes>);

// In order to put it into a numpy array
impl Element for PyBytes {
    const DATA_TYPE: numpy::DataType = numpy::DataType::Object;
    fn is_same_type(dtype: &PyArrayDescr) -> bool {
        unsafe { *dtype.as_dtype_ptr() }.type_num == NPY_TYPES::NPY_OBJECT as i32
    }
}

pub struct BytesBlock<'a> {
    data: ArrayViewMut2<'a, PyBytes>,
    buf_size_mb: usize,
}

impl<'a> FromPyObject<'a> for BytesBlock<'a> {
    fn extract(ob: &'a PyAny) -> PyResult<Self> {
        check_dtype(ob, "object")?;
        let array = ob.downcast::<PyArray<PyBytes, Ix2>>()?;
        let data = unsafe { array.as_array_mut() };
        Ok(BytesBlock {
            data,
            buf_size_mb: 16, // in MB
        })
    }
}

impl<'a> BytesBlock<'a> {
    #[throws(ConnectorXPythonError)]
    pub fn split(self) -> Vec<BytesColumn<'a>> {
        let mut ret = vec![];
        let mut view = self.data;

        let nrows = view.ncols();
        while view.nrows() > 0 {
            let (col, rest) = view.split_at(Axis(0), 1);
            view = rest;
            ret.push(BytesColumn {
                data: col
                    .into_shape(nrows)?
                    .into_slice()
                    .ok_or_else(|| anyhow!("get None for splitted String data"))?,
                next_write: 0,
                bytes_lengths: vec![],
                bytes_buf: Vec::with_capacity(self.buf_size_mb * (1 << 20) * 11 / 10), // allocate a little bit more memory to avoid Vec growth
                buf_size: self.buf_size_mb * (1 << 20),
            })
        }
        ret
    }
}

pub struct BytesColumn<'a> {
    data: &'a mut [PyBytes],
    next_write: usize,
    bytes_buf: Vec<u8>,
    bytes_lengths: Vec<usize>, // usize::MAX if the string is None
    buf_size: usize,
}

impl<'a> PandasColumnObject for BytesColumn<'a> {
    fn typecheck(&self, id: TypeId) -> bool {
        id == TypeId::of::<&'static [u8]>() || id == TypeId::of::<Option<&'static [u8]>>()
    }
    fn len(&self) -> usize {
        self.data.len()
    }
    fn typename(&self) -> &'static str {
        std::any::type_name::<&'static [u8]>()
    }
    #[throws(ConnectorXPythonError)]
    fn finalize(&mut self) {
        self.flush()?;
    }
}

impl<'a> PandasColumn<Vec<u8>> for BytesColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Vec<u8>) {
        self.bytes_lengths.push(val.len());
        self.bytes_buf.extend_from_slice(&val[..]);
        self.try_flush()?;
    }
}

impl<'r, 'a> PandasColumn<&'r [u8]> for BytesColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: &'r [u8]) {
        self.bytes_lengths.push(val.len());
        self.bytes_buf.extend_from_slice(val);
        self.try_flush()?;
    }
}

impl<'a> PandasColumn<Option<Vec<u8>>> for BytesColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<Vec<u8>>) {
        match val {
            Some(b) => {
                self.bytes_lengths.push(b.len());
                self.bytes_buf.extend_from_slice(&b[..]);
                self.try_flush()?;
            }
            None => {
                self.bytes_lengths.push(usize::MAX);
            }
        }
    }
}

impl<'r, 'a> PandasColumn<Option<&'r [u8]>> for BytesColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<&'r [u8]>) {
        match val {
            Some(b) => {
                self.bytes_lengths.push(b.len());
                self.bytes_buf.extend_from_slice(b);
                self.try_flush()?;
            }
            None => {
                self.bytes_lengths.push(usize::MAX);
            }
        }
    }
}

impl HasPandasColumn for Vec<u8> {
    type PandasColumn<'a> = BytesColumn<'a>;
}

impl HasPandasColumn for Option<Vec<u8>> {
    type PandasColumn<'a> = BytesColumn<'a>;
}

impl<'r> HasPandasColumn for &'r [u8] {
    type PandasColumn<'a> = BytesColumn<'a>;
}

impl<'r> HasPandasColumn for Option<&'r [u8]> {
    type PandasColumn<'a> = BytesColumn<'a>;
}

impl<'a> BytesColumn<'a> {
    pub fn partition(self, counts: &[usize]) -> Vec<BytesColumn<'a>> {
        let mut partitions = vec![];
        let mut data = self.data;

        for &c in counts {
            let (splitted_data, rest) = data.split_at_mut(c);
            data = rest;

            partitions.push(BytesColumn {
                data: splitted_data,
                next_write: 0,
                bytes_lengths: vec![],
                bytes_buf: Vec::with_capacity(self.buf_size),
                buf_size: self.buf_size,
            });
        }

        partitions
    }

    #[throws(ConnectorXPythonError)]
    pub fn flush(&mut self) {
        let nstrings = self.bytes_lengths.len();

        if nstrings > 0 {
            let py = unsafe { Python::assume_gil_acquired() };

            {
                // allocation in python is not thread safe
                let _guard = GIL_MUTEX
                    .lock()
                    .map_err(|e| anyhow!("mutex poisoned {}", e))?;
                let mut start = 0;
                for (i, &len) in self.bytes_lengths.iter().enumerate() {
                    if len != usize::MAX {
                        let end = start + len;
                        unsafe {
                            // allocate and write in the same time
                            *self.data.get_unchecked_mut(self.next_write + i) = PyBytes(
                                pyo3::types::PyBytes::new(py, &self.bytes_buf[start..end]).into(),
                            );
                        };
                        start = end;
                    } else {
                        unsafe {
                            let b: &pyo3::types::PyBytes =
                                py.from_borrowed_ptr(pyo3::ffi::Py_None());

                            *self.data.get_unchecked_mut(self.next_write + i) = PyBytes(b.into());
                        }
                    }
                }
            }

            self.bytes_buf.truncate(0);
            self.bytes_lengths.truncate(0);
            self.next_write += nstrings;
        }
    }

    #[throws(ConnectorXPythonError)]
    pub fn try_flush(&mut self) {
        if self.bytes_buf.len() >= self.buf_size {
            self.flush()?;
        }
    }
}
