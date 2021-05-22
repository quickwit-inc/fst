use std::{
    fmt::Debug,
    ops::{Bound, RangeBounds},
};
use std::{
    io::Read,
    ops::{Index, Range, RangeFrom, RangeFull, RangeToInclusive},
};

pub type Ulen = u64; // maybe changeable? shouldn't be Ulen since then we couldn't use an index > 2GB in webassembly

pub fn full_slice(b: &dyn FakeArr) -> FakeArrSlice<'_> {
    return FakeArrSlice {
        real: Wtfisthis::Dyn(b),
        offset: 0,
        len: b.len(),
    };
}

// a FakeArr can either be a real array (based on a Vec<u8>, a &[u8], or a memmap), or it can be a file read on demand by OS read() calls
pub trait FakeArr: Debug {
    fn len(&self) -> Ulen;
    fn read_into(&self, offset: Ulen, buf: &mut [u8]) -> std::io::Result<()>;
    fn get_ofs_len(&self, start: Bound<Ulen>, end: Bound<Ulen>) -> (Ulen, Ulen) {
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
    fn slice<'a>(&'a self, bounds: ShRange<Ulen>) -> FakeArrSlice<'a> {
        let (offset, len) = self.get_ofs_len(bounds.0, bounds.1);
        FakeArrSlice {
            real: Wtfisthis::Dyn(self.as_dyn()),
            offset,
            len,
        }
    }
    fn full_slice(&self) -> FakeArrSlice<'_> {
        self.slice((..).into())
    }
    fn get_byte(&self, offset: Ulen) -> u8 {
        self.slice((offset..offset + 1).into()).actually_read_it()[0]
    }
    fn actually_read_it(&self) -> Vec<u8> {
        let mut v = vec![0; self.len() as usize];
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
// todo: is there any better way?
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
enum Wtfisthis<'a> {
    Dyn(&'a dyn FakeArr),
    Slic(&'a [u8]),
}
impl<'a> Wtfisthis<'a> {
    fn as_dyn(&self) -> &dyn FakeArr {
        match &self {
            Wtfisthis::Dyn(e) => *e,
            Wtfisthis::Slic(e) => e.as_dyn(),
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct FakeArrSlice<'a> {
    real: Wtfisthis<'a>,
    offset: Ulen,
    len: Ulen,
}
impl<'a> FakeArrSlice<'a> {
    pub fn get_offset(&self) -> Ulen {
        self.offset
    }
    // the same as .slice, but returns a thing of the lifetime of the root real fake array instead of this part so the returned part can live longer than this one
    pub fn slice2(&self, b: ShRange<Ulen>) -> FakeArrSlice<'a> {
        let (start, len) = self.get_ofs_len(b.0, b.1);
        return FakeArrSlice {
            real: self.real,
            offset: self.offset + start,
            len,
        };
    }
}
impl Read for FakeArrSlice<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read_len = std::cmp::min(buf.len() as Ulen, self.len);
        let res = (*self).read_into(0, buf).map(|()| read_len as usize);
        self.offset += read_len;
        self.len -= read_len;
        res
    }
}

impl<'a> FakeArr for FakeArrSlice<'a> {
    fn len(&self) -> Ulen {
        self.len
    }

    fn read_into(&self, offset: Ulen, buf: &mut [u8]) -> std::io::Result<()> {
        self.real.as_dyn().read_into(self.offset + offset, buf)
    }

    fn slice(&self, b: ShRange<Ulen>) -> FakeArrSlice<'a> {
        self.slice2(b)
    }

    fn as_dyn(&self) -> &dyn FakeArr {
        todo!()
    }
}

pub type FakeArrRef<'a> = FakeArrSlice<'a>;


impl FakeArr for Vec<u8> {
    fn len(&self) -> Ulen {
        self.len() as Ulen
    }

    fn read_into(&self, offset: Ulen, buf: &mut [u8]) -> std::io::Result<()> {
        <&[u8] as FakeArr>::read_into(&&self[..], offset, buf)
    }

    fn as_dyn(&self) -> &dyn FakeArr {
        self
    }
}

impl FakeArr for &[u8] {
    fn len(&self) -> Ulen {
        return (self as &[u8]).len() as Ulen;
    }

    fn read_into(&self, offset: Ulen, buf: &mut [u8]) -> std::io::Result<()> {
        let end = offset as usize + buf.len();
        buf.copy_from_slice(&self[offset as usize..end]);
        Ok(())
    }
    fn as_dyn(&self) -> &dyn FakeArr {
        self
    }
}

const EMPTY1: &[u8; 0] = &[];

pub fn empty() -> FakeArrSlice<'static> {
    let x = FakeArrSlice {
        real: Wtfisthis::Slic(EMPTY1),
        offset: 0,
        len: 0,
    };
    x
}

pub fn slice_to_fake_arr<'a>(slice: &'a [u8]) -> FakeArrRef<'a> {
    FakeArrSlice {
        real: Wtfisthis::Slic(slice),
        offset: 0,
        len: slice.len() as Ulen,
    }
}
