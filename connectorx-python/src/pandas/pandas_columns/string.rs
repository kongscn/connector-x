use super::super::pystring::{PyString, StringInfo};
use super::{check_dtype, HasPandasColumn, PandasColumn, PandasColumnObject, GIL_MUTEX};
use crate::errors::ConnectorXPythonError;
use anyhow::anyhow;
use fehler::throws;
use itertools::Itertools;
use ndarray::{ArrayViewMut2, Axis, Ix2};
use numpy::PyArray;
use pyo3::{FromPyObject, PyAny, PyResult, Python};
use std::any::TypeId;

pub struct StringBlock<'a> {
    data: ArrayViewMut2<'a, PyString>,
    buf_size_mb: usize,
}

impl<'a> FromPyObject<'a> for StringBlock<'a> {
    fn extract(ob: &'a PyAny) -> PyResult<Self> {
        check_dtype(ob, "object")?;
        let array = ob.downcast::<PyArray<PyString, Ix2>>()?;
        let data = unsafe { array.as_array_mut() };
        Ok(StringBlock {
            data,
            buf_size_mb: 4, // in MB
        })
    }
}

impl<'a> StringBlock<'a> {
    #[throws(ConnectorXPythonError)]
    pub fn split(self) -> Vec<StringColumn<'a>> {
        let mut ret = vec![];
        let mut view = self.data;

        let nrows = view.ncols();
        while view.nrows() > 0 {
            let (col, rest) = view.split_at(Axis(0), 1);
            view = rest;
            ret.push(StringColumn {
                data: col
                    .into_shape(nrows)?
                    .into_slice()
                    .ok_or_else(|| anyhow!("get None for splitted String data"))?,
                next_write: 0,
                string_lengths: vec![],
                string_buf: Vec::with_capacity(self.buf_size_mb * (1 << 20) * 11 / 10), // allocate a little bit more memory to avoid Vec growth
                buf_size: self.buf_size_mb * (1 << 20),
            })
        }
        ret
    }
}

pub struct StringColumn<'a> {
    data: &'a mut [PyString],
    next_write: usize,
    string_buf: Vec<u8>,
    string_lengths: Vec<usize>, // usize::MAX for empty string
    buf_size: usize,
}

impl<'a> PandasColumnObject for StringColumn<'a> {
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
        self.flush(true)?;
    }
}

impl<'r, 'a> PandasColumn<&'r str> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: &'r str) {
        let bytes = val.as_bytes();
        self.string_lengths.push(bytes.len());
        self.string_buf.extend_from_slice(bytes);
        self.try_flush()?;
    }
}

impl<'a> PandasColumn<Box<str>> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Box<str>) {
        let bytes = val.as_bytes();
        self.string_lengths.push(bytes.len());
        self.string_buf.extend_from_slice(bytes);
        self.try_flush()?;
    }
}

impl<'a> PandasColumn<String> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: String) {
        let bytes = val.as_bytes();
        self.string_lengths.push(bytes.len());
        self.string_buf.extend_from_slice(bytes);
        self.try_flush()?;
    }
}

impl<'a> PandasColumn<char> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: char) {
        let mut buffer = [0; 4]; // a char is max to 4 bytes
        let bytes = val.encode_utf8(&mut buffer).as_bytes();
        self.string_lengths.push(bytes.len());
        self.string_buf.extend_from_slice(bytes);
        self.try_flush()?;
    }
}

impl<'r, 'a> PandasColumn<Option<&'r str>> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<&'r str>) {
        match val {
            Some(b) => {
                let bytes = b.as_bytes();
                self.string_lengths.push(bytes.len());
                self.string_buf.extend_from_slice(bytes);
                self.try_flush()?;
            }
            None => {
                self.string_lengths.push(usize::MAX);
            }
        }
    }
}

impl<'a> PandasColumn<Option<Box<str>>> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<Box<str>>) {
        match val {
            Some(b) => {
                let bytes = b.as_bytes();
                self.string_lengths.push(bytes.len());
                self.string_buf.extend_from_slice(bytes);
                self.try_flush()?;
            }
            None => {
                self.string_lengths.push(usize::MAX);
            }
        }
    }
}
impl<'a> PandasColumn<Option<String>> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<String>) {
        match val {
            Some(b) => {
                let bytes = b.as_bytes();
                self.string_lengths.push(bytes.len());
                self.string_buf.extend_from_slice(bytes);
                self.try_flush()?;
            }
            None => {
                self.string_lengths.push(usize::MAX);
            }
        }
    }
}

impl<'a> PandasColumn<Option<char>> for StringColumn<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<char>) {
        match val {
            Some(b) => {
                let mut buffer = [0; 4]; // a char is max to 4 bytes
                let bytes = b.encode_utf8(&mut buffer).as_bytes();
                self.string_lengths.push(bytes.len());
                self.string_buf.extend_from_slice(bytes);
                self.try_flush()?;
            }
            None => {
                self.string_lengths.push(usize::MAX);
            }
        }
    }
}

impl<'r> HasPandasColumn for &'r str {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl<'r> HasPandasColumn for Option<&'r str> {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl HasPandasColumn for String {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl HasPandasColumn for Option<String> {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl HasPandasColumn for char {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl HasPandasColumn for Option<char> {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl HasPandasColumn for Box<str> {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl HasPandasColumn for Option<Box<str>> {
    type PandasColumn<'a> = StringColumn<'a>;
}

impl<'a> StringColumn<'a> {
    pub fn partition(self, counts: &[usize]) -> Vec<StringColumn<'a>> {
        let mut partitions = vec![];
        let mut data = self.data;

        for &c in counts {
            let (splitted_data, rest) = data.split_at_mut(c);
            data = rest;

            partitions.push(StringColumn {
                data: splitted_data,
                next_write: 0,
                string_lengths: vec![],
                string_buf: Vec::with_capacity(self.buf_size),
                buf_size: self.buf_size,
            });
        }

        partitions
    }

    #[throws(ConnectorXPythonError)]
    pub fn flush(&mut self, force: bool) {
        let nstrings = self.string_lengths.len();
        if nstrings == 0 {
            return;
        }

        let py = unsafe { Python::assume_gil_acquired() };
        let _guard = if force {
            GIL_MUTEX
                .lock()
                .map_err(|e| anyhow!("mutex poisoned {}", e))?
        } else {
            match GIL_MUTEX.try_lock() {
                Ok(guard) => guard,
                Err(_) => return,
            }
        };

        let mut string_infos = Vec::with_capacity(self.string_lengths.len());
        let mut start = 0;
        for (i, &len) in self.string_lengths.iter().enumerate() {
            if len != usize::MAX {
                let end = start + len;

                unsafe {
                    let string_info = StringInfo::detect(&self.string_buf[start..end]);
                    *self.data.get_unchecked_mut(self.next_write + i) = string_info.pystring(py);
                    string_infos.push(Some(string_info));
                };

                start = end;
            } else {
                string_infos.push(None);

                unsafe { *self.data.get_unchecked_mut(self.next_write + i) = PyString::none(py) };
            }
        }

        if !string_infos.is_empty() {
            let mut start = 0;
            for (i, (len, info)) in self
                .string_lengths
                .drain(..)
                .zip_eq(string_infos)
                .enumerate()
            {
                if len != usize::MAX {
                    let end = start + len;
                    unsafe {
                        self.data[self.next_write + i]
                            .write(&self.string_buf[start..end], info.unwrap())
                    };
                    start = end;
                }
            }

            self.string_buf.truncate(0);
            self.next_write += nstrings;
        }
    }

    #[throws(ConnectorXPythonError)]
    pub fn try_flush(&mut self) {
        if self.string_buf.len() >= self.buf_size {
            self.flush(true)?;
            return;
        }
        #[cfg(feature = "nbstr")]
        if self.string_buf.len() >= self.buf_size / 2 {
            self.flush(false)?;
        }
    }
}
