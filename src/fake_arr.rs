use std::fmt::Debug;
use std::{
    io::Read,
    ops::{Index, Range, RangeFrom, RangeFull, RangeToInclusive},
};

pub trait FakeArr: Debug {
    fn len(&self) -> usize;
    fn read_into(&self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn actually_read_it(&self) -> Vec<u8> {
        let mut v = vec![0; self.len()];
        self.read_into(&mut v).unwrap();
        v
    }
    fn to_vec(&self) -> Vec<u8> {
        self.actually_read_it()
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl<'a> PartialEq for dyn FakeArr + 'a {
    fn eq(&self, other: &Self) -> bool {
        return &self.to_vec()[..] == &other.to_vec()[..];
    }
}
impl Read for &dyn FakeArr {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (*self).read_into(buf)
    }
}

impl<'a> Index<usize> for FakeArr + 'a {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        let mut out: u8 = 0;
        self[index..index + 1].read_into(std::slice::from_mut(&mut out));
        return &out;
    }
}
impl<'a> Index<RangeFrom<usize>> for FakeArr + 'a {
    type Output = FakeArr + 'a;

    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        todo!()
    }
}
impl<'a> Index<RangeToInclusive<usize>> for FakeArr + 'a {
    type Output = FakeArr + 'a;

    fn index(&self, index: RangeToInclusive<usize>) -> &Self::Output {
        todo!()
    }
}
impl<'a> Index<Range<usize>> for FakeArr + 'a {
    type Output = FakeArr + 'a;

    fn index(&self, index: Range<usize>) -> &Self::Output {
        todo!()
    }
}

pub type FakeArrRef<'a> = &'a (dyn FakeArr);
/*#[derive(Clone, Copy)]
pub struct FakeArrRef<'a> {
    arr: &'a dyn FakeArr,
}
impl FakeArrRef<'_> {
    fn new(arr: &dyn FakeArr) -> FakeArrRef {
        FakeArrRef { arr }
    }
}

impl<'a> std::ops::Deref for FakeArrRef<'a> {
    type Target = dyn FakeArr + 'a;

    fn deref(&self) -> &Self::Target {
        self.arr
    }
}

impl Index<usize> for FakeArrRef<'_> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        todo!()
    }
}
impl Index<RangeFrom<usize>> for FakeArrRef<'_> {
    type Output = [u8];

    fn index(&self, index: RangeFrom<usize>) -> &Self::Output {
        todo!()
    }
}
impl Index<RangeToInclusive<usize>> for FakeArrRef<'_> {
    type Output = [u8];

    fn index(&self, index: RangeToInclusive<usize>) -> &Self::Output {
        todo!()
    }
}
impl Index<Range<usize>> for FakeArrRef<'_> {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &Self::Output {
        todo!()
    }
}*/

/*impl Index<RangeFull<usize>> for FakeArr {
    type Output = [u8];
}*/

impl FakeArr for Vec<u8> {
    fn len(&self) -> usize {
        self.len()
    }

    fn actually_read_it(&self) -> Vec<u8> {
        return self.to_vec();
    }

    fn read_into(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        (&self[..]).read(buf)
    }
}

impl FakeArr for [u8] {
    fn len(&self) -> usize {
        return self.len();
    }

    fn actually_read_it(&self) -> Vec<u8> {
        return self.to_vec();
    }

    fn read_into(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        panic!();
        // std::io::Read::read(&mut self, buf)
    }
}

pub const EMPTY: &'static dyn FakeArr = &vec![];

struct FakeArrFromSlice<'a> {
    slice: &'a [u8],
}

pub fn slice_to_fake_arr<'a>(slice: &'a [u8]) -> FakeArrRef<'a> {
    //let s = std::mem::leak(slice);
    panic!();
    // slice
}
/*pub fn slice_to_fake_arr(slice: &[u8]) -> FakeArrFromSlice {
    FakeArrFromSlice { slice }
}*/
/*impl<'a> FakeArr for FakeArrFromSlice<'a> {
    fn len(&self) -> usize {
        todo!()
    }

    fn actually_read_it(&self) -> Vec<u8> {
        todo!()
    }
}
*/
