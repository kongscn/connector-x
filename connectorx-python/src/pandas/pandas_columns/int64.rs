use super::{check_dtype, HasPandasColumn, PandasColumn, PandasColumnObject};
use crate::errors::ConnectorXPythonError;
use anyhow::anyhow;
use fehler::throws;
use ndarray::{ArrayViewMut1, ArrayViewMut2, Axis, Ix2};
use numpy::{PyArray, PyArray1};
use pyo3::{types::PyTuple, FromPyObject, PyAny, PyResult};
use std::any::TypeId;

pub enum Int64Block<'a> {
    NumPy(ArrayViewMut2<'a, i64>),
    Extention(ArrayViewMut1<'a, i64>, ArrayViewMut1<'a, bool>),
}
impl<'a> FromPyObject<'a> for Int64Block<'a> {
    fn extract(ob: &'a PyAny) -> PyResult<Self> {
        if let Ok(array) = ob.downcast::<PyArray<i64, Ix2>>() {
            check_dtype(ob, "int64")?;
            let data = unsafe { array.as_array_mut() };
            Ok(Int64Block::NumPy(data))
        } else {
            let tuple = ob.downcast::<PyTuple>()?;
            let data = tuple.get_item(0);
            let mask = tuple.get_item(1);
            check_dtype(data, "int64")?;
            check_dtype(mask, "bool")?;

            Ok(Int64Block::Extention(
                unsafe { data.downcast::<PyArray1<i64>>()?.as_array_mut() },
                unsafe { mask.downcast::<PyArray1<bool>>()?.as_array_mut() },
            ))
        }
    }
}

impl<'a> Int64Block<'a> {
    #[throws(ConnectorXPythonError)]
    pub fn split(self) -> Vec<Int64Column<'a>> {
        let mut ret = vec![];
        match self {
            Int64Block::Extention(data, mask) => ret.push(Int64Column {
                data: data
                    .into_slice()
                    .ok_or_else(|| anyhow!("get None for Int64 data"))?,
                mask: Some(
                    mask.into_slice()
                        .ok_or_else(|| anyhow!("get None for Int64 mask"))?,
                ),
                i: 0,
            }),
            Int64Block::NumPy(mut view) => {
                let nrows = view.ncols();
                while view.nrows() > 0 {
                    let (col, rest) = view.split_at(Axis(0), 1);
                    view = rest;
                    ret.push(Int64Column {
                        data: col
                            .into_shape(nrows)?
                            .into_slice()
                            .ok_or_else(|| anyhow!("get None for splitted Int64 data"))?,
                        mask: None,
                        i: 0,
                    })
                }
            }
        }
        ret
    }
}

// for uint64 and Int64
pub struct Int64Column<'a> {
    data: &'a mut [i64],
    mask: Option<&'a mut [bool]>,
    i: usize,
}

impl<'a> PandasColumnObject for Int64Column<'a> {
    fn typecheck(&self, id: TypeId) -> bool {
        id == TypeId::of::<i64>() || id == TypeId::of::<Option<i64>>()
    }
    fn len(&self) -> usize {
        self.data.len()
    }
    fn typename(&self) -> &'static str {
        std::any::type_name::<i64>()
    }
}

impl<'a> PandasColumn<i64> for Int64Column<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: i64) {
        unsafe { *self.data.get_unchecked_mut(self.i) = val };
        if let Some(mask) = self.mask.as_mut() {
            unsafe { *mask.get_unchecked_mut(self.i) = false };
        }
        self.i += 1;
    }
}

impl<'a> PandasColumn<Option<i64>> for Int64Column<'a> {
    #[throws(ConnectorXPythonError)]
    fn write(&mut self, val: Option<i64>) {
        match val {
            Some(val) => {
                unsafe { *self.data.get_unchecked_mut(self.i) = val };
                if let Some(mask) = self.mask.as_mut() {
                    unsafe { *mask.get_unchecked_mut(self.i) = false };
                }
            }
            None => {
                if let Some(mask) = self.mask.as_mut() {
                    unsafe { *mask.get_unchecked_mut(self.i) = true };
                } else {
                    panic!("Writing null i64 to not null pandas array")
                }
            }
        }
        self.i += 1;
    }
}

impl HasPandasColumn for i64 {
    type PandasColumn<'a> = Int64Column<'a>;
}

impl HasPandasColumn for Option<i64> {
    type PandasColumn<'a> = Int64Column<'a>;
}

impl<'a> Int64Column<'a> {
    pub fn partition(self, counts: &[usize]) -> Vec<Int64Column<'a>> {
        let mut partitions = vec![];
        let mut data = self.data;
        let mut mask = self.mask;

        for &c in counts {
            let (splitted_data, rest) = data.split_at_mut(c);
            data = rest;
            let (splitted_mask, rest) = match mask {
                Some(mask) => {
                    let (a, b) = mask.split_at_mut(c);
                    (Some(a), Some(b))
                }
                None => (None, None),
            };

            mask = rest;

            partitions.push(Int64Column {
                data: splitted_data,
                mask: splitted_mask,
                i: 0,
            });
        }

        partitions
    }
}
