use std::{
    fmt::Debug,
    ops::{Bound, RangeBounds},
};
use std::{
    io::Read,
    ops::{Index, Range, RangeFrom, RangeFull, RangeToInclusive},
};

pub fn full_slice(b: &dyn FakeArr) -> FakeArrPart<'_> {
    return FakeArrPart {
        real: Fuckyou::Dyn(b),
        offset: 0,
        len: b.len(),
    };
}
pub trait FakeArr: Debug {
    fn len(&self) -> usize;
    fn read_into(&self, offset: usize, buf: &mut [u8]) -> std::io::Result<()>;
    fn get_ofs_len(&self, start: Bound<usize>, end: Bound<usize>) -> (usize, usize) {
        use Bound::*;
        let start = match start {
            Unbounded => 0,
            Included(i) => i,
            Excluded(i) => panic!(),
        };
        let end = match end {
            Unbounded => self.len(),
            Included(i) => i + 1,
            Excluded(i) => i,
        };
        return (start, end - start);
    }
    /*fn slice_w_range(&self, e: SomRang) -> FakeArrPart<'_> {

    }*/
    fn slice<'a>(&'a self, bounds: ShRange<usize>) -> FakeArrPart<'a> {
        let (offset, len) = self.get_ofs_len(bounds.0, bounds.1);
        FakeArrPart {
            real: Fuckyou::Dyn(self.as_dyn()),
            offset,
            len,
        }
    }
    fn full_slice(&self) -> FakeArrPart<'_> {
        self.slice((..).into())
    }
    fn get_byte(&self, offset: usize) -> u8 {
        self.slice((offset..offset + 1).into()).actually_read_it()[0]
    }
    fn actually_read_it(&self) -> Vec<u8> {
        let mut v = vec![0; self.len()];
        self.read_into(0, &mut v).unwrap();
        v
    }
    fn to_vec(&self) -> Vec<u8> {
        self.actually_read_it()
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn as_dyn(&self) -> &dyn FakeArr;
}
impl<'a> PartialEq for dyn FakeArr + 'a {
    fn eq(&self, other: &Self) -> bool {
        return &self.to_vec()[..] == &other.to_vec()[..];
    }
}
impl Read for &dyn FakeArr {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        (*self).read_into(0, buf).map(|()| buf.len())
    }
}

#[macro_export]
macro_rules! slic {
    ($($e:ident).+ [$x:tt..]) => (($($e).*).slice(($x..).into()));
    ($($e:ident).+ [$x:tt..$y:tt]) => (($($e).*).slice(($x..$y).into()));
    ($($e:ident).+ [..=$y:tt]) => (($($e).*).slice((..=$y).into()));
    ($($e:ident).+ [..]) =>(($($e).*).slice((..).into()));
    ($($e:ident).+ [$x:tt]) => (($($e).*).get_byte($x));
}
#[macro_export]
macro_rules! slic2 {
    ($($e:ident).+ [$x:tt..]) => (($($e).*).slice2(($x..).into()));
    ($($e:ident).+ [$x:tt..$y:tt]) => (($($e).*).slice2(($x..$y).into()));
    ($($e:ident).+ [..=$y:tt]) => (($($e).*).slice2((..=$y).into()));
    ($($e:ident).+ [..]) =>(($($e).*).slice2((..).into()));
    ($($e:ident).+ [$x:tt]) => (($($e).*).get_byte($x));
}
pub struct ShRange<T>(Bound<T>, Bound<T>);

fn bound_cloned<T: Clone>(b: Bound<&T>) -> Bound<T> {
    match b {
        Bound::Unbounded => Bound::Unbounded,
        Bound::Included(x) => Bound::Included(x.clone()),
        Bound::Excluded(x) => Bound::Excluded(x.clone()),
    }
}
impl<T: Clone> From<Range<T>> for ShRange<T> {
    fn from(r: Range<T>) -> Self {
        ShRange(bound_cloned(r.start_bound()), bound_cloned(r.end_bound()))
    }
}
impl<T: Clone> From<RangeFull> for ShRange<T> {
    fn from(r: RangeFull) -> Self {
        ShRange(bound_cloned(r.start_bound()), bound_cloned(r.end_bound()))
    }
}
impl<T: Clone> From<RangeFrom<T>> for ShRange<T> {
    fn from(r: RangeFrom<T>) -> Self {
        ShRange(bound_cloned(r.start_bound()), bound_cloned(r.end_bound()))
    }
}
impl<T: Clone> From<RangeToInclusive<T>> for ShRange<T> {
    fn from(r: RangeToInclusive<T>) -> Self {
        ShRange(bound_cloned(r.start_bound()), bound_cloned(r.end_bound()))
    }
}

#[derive(Debug, Clone, Copy)]
// idk why, but i can't figure out why this can't just be &'a dyn FakeArr
enum Fuckyou<'a> {
    Dyn(&'a dyn FakeArr),
    Slic(&'a [u8]),
}
impl<'a> Fuckyou<'a> {
    fn as_dyn(&self) -> &dyn FakeArr {
        match &self {
            Fuckyou::Dyn(e) => *e,
            Fuckyou::Slic(e) => e.as_dyn(),
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct FakeArrPart<'a> {
    real: Fuckyou<'a>,
    offset: usize,
    len: usize,
}
impl<'a> FakeArrPart<'a> {
    // the same as .slice, but returns a thing of the lifetime of the root real fake array instead of this part so the returned part can live longer than this one
    pub fn slice2(&self, b: ShRange<usize>) -> FakeArrPart<'a> {
        let (start, len) = self.get_ofs_len(b.0, b.1);
        return FakeArrPart {
            real: self.real,
            offset: self.offset + start,
            len,
        };
    }
}
impl<'a> FakeArr for FakeArrPart<'a> {
    fn len(&self) -> usize {
        self.len
    }

    fn read_into(&self, offset: usize, buf: &mut [u8]) -> std::io::Result<()> {
        self.real.as_dyn().read_into(self.offset + offset, buf)
    }

    /*fn get_byte(&self, offset: usize) -> u8 {
        self.real.as_dyn().get_byte(self.offset + offset)
    }*/

    fn slice(&self, b: ShRange<usize>) -> FakeArrPart<'a> {
        self.slice2(b)
    }

    fn as_dyn(&self) -> &dyn FakeArr {
        todo!()
    }
}

pub type FakeArrRef<'a> = FakeArrPart<'a>;
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

    fn read_into(&self, offset: usize, buf: &mut [u8]) -> std::io::Result<()> {
        buf.copy_from_slice(&self[offset..offset + buf.len()]);
        Ok(())
    }

    /*fn get_byte(&self, offset: usize) -> u8 {
        self[offset]
    }*/

    fn as_dyn(&self) -> &dyn FakeArr {
        self
    }
}

impl FakeArr for &[u8] {
    fn len(&self) -> usize {
        return (self as &[u8]).len();
    }

    fn read_into(&self, offset: usize, buf: &mut [u8]) -> std::io::Result<()> {
        buf.copy_from_slice(&self[offset..offset + buf.len()]);
        Ok(())
    }

    /*fn get_byte(&self, offset: usize) -> u8 {
        self[offset]
    }*/

    /*fn slice(&self, bounds: ShRange<usize>) -> FakeArrPart<'_> {
        let (start, len) = self.get_ofs_len(bounds.0, bounds.1);
        slice_to_fake_arr(&self[start..start + len])
    }*/
    fn as_dyn(&self) -> &dyn FakeArr {
        self
    }
}

/*#[derive(Debug, Clone, Copy)]
pub struct FakeArrSlice<'a> {
    real: &'a [u8],
    offset: usize,
    len: usize,
}
impl<'a> FakeArr for FakeArrSlice<'a> {
    fn len(&self) -> usize {
        todo!()
    }

    fn read_into(&self, offset: usize, buf: &mut [u8]) -> std::io::Result<usize> {
        todo!()
    }

    fn slice(&self, b: ShRange<usize>) -> FakeArrPart {
        todo!()
    }

    fn get_byte(&self, offset: usize) -> u8 {
        todo!()
    }

    fn as_dyn(&self) -> &dyn FakeArr {
        todo!()
    }

}*/

const EMPTY1: &[u8; 0] = &[];
/*pub const EMPTY2: FakeArrSlice = FakeArrSlice {
    real: EMPTY1,
    offset: 0,
    len: 0,
};*/
pub fn empty() -> FakeArrPart<'static> {
    let x = FakeArrPart {
        real: Fuckyou::Slic(EMPTY1),
        offset: 0,
        len: 0,
    };
    x
}

pub fn slice_to_fake_arr<'a>(slice: &'a [u8]) -> FakeArrRef<'a> {
    // slice.as_dyn().slice((..).into())
    println!("slice_to_fake_arr: {:?}", slice);
    FakeArrPart {
        real: Fuckyou::Slic(slice),
        offset: 0,
        len: slice.len(),
    }
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
