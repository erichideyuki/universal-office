//! The bind emitter (src/rust_bind.rs) on its fixture (schemas/bind_test.uos + schemas/bind_test.bind →
//! fixtures/bind_test_bind.rs): random wire messages go wire → native → wire and come back identical (by
//! the canonical dump: NaN equals itself there), every binding kind included; a native-only variant has
//! no wire form; the `default` field is filled in.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

#[path = "fixtures/bind_native.rs"]
mod bind_native;

mod bind_fixture {
    // generated code: the emitters' own lint allowances (as uo-wasm includes them)
    #![allow(dead_code, clippy::all)]
    #[path = "../fixtures/bind_test_bind.rs"]
    mod bind;
    #[path = "../fixtures/bind_test.rs"]
    pub mod bind_test;
    #[path = "../fixtures/bind_test_support.rs"]
    pub mod generated_support;
}

/// The traits the generated file implements and the leaf conversions it relies on (a consumer's
/// runtime; the session's is crates/uo-wasm/src/session/wire.rs).
mod bind_runtime {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BindError {
        NativeOnly(&'static str),
    }
    pub trait FromWire<W>: Sized {
        fn from_wire(w: W) -> Result<Self, BindError>;
    }
    pub trait IntoWire<W> {
        #[allow(clippy::wrong_self_convention)]
        fn into_wire(&self) -> Result<W, BindError>;
    }
    macro_rules! same {
        ($($t:ty),*) => {$(
            impl FromWire<$t> for $t { fn from_wire(w: $t) -> Result<Self, BindError> { Ok(w) } }
            impl IntoWire<$t> for $t { fn into_wire(&self) -> Result<$t, BindError> { Ok(*self) } }
        )*};
    }
    same!(u32, f64);
    impl FromWire<Vec<u16>> for String {
        fn from_wire(w: Vec<u16>) -> Result<Self, BindError> {
            Ok(String::from_utf16_lossy(&w))
        }
    }
    impl IntoWire<Vec<u16>> for String {
        fn into_wire(&self) -> Result<Vec<u16>, BindError> {
            Ok(self.encode_utf16().collect())
        }
    }
    impl<T: FromWire<W>, W> FromWire<Option<W>> for Option<T> {
        fn from_wire(w: Option<W>) -> Result<Self, BindError> {
            w.map(T::from_wire).transpose()
        }
    }
    impl<T: IntoWire<W>, W> IntoWire<Option<W>> for Option<T> {
        fn into_wire(&self) -> Result<Option<W>, BindError> {
            self.as_ref().map(IntoWire::into_wire).transpose()
        }
    }
    impl<T: FromWire<W>, W> FromWire<Vec<W>> for Vec<T> {
        fn from_wire(w: Vec<W>) -> Result<Self, BindError> {
            w.into_iter().map(T::from_wire).collect()
        }
    }
    impl<T: IntoWire<W>, W> IntoWire<Vec<W>> for Vec<T> {
        fn into_wire(&self) -> Result<Vec<W>, BindError> {
            self.iter().map(IntoWire::into_wire).collect()
        }
    }
}

use bind_fixture::bind_test as w;
use bind_fixture::generated_support::{Bounds, dump_bag};
use bind_native::{Bag, Color, Point, Report, Shape, Tagged};
use bind_runtime::{BindError, FromWire, IntoWire};
use uo_codec::testing::Rng;

/// A random wire string with a lone surrogate is a lossy native `String`: the fixture's strings are made
/// well-formed first (the session declares the same narrowing, WS-D2).
fn well_formed(b: &mut w::Bag) {
    let fix = |u: &mut Vec<u16>| *u = String::from_utf16_lossy(u).encode_utf16().collect();
    let point = |p: &mut w::Point| {
        if let Some(l) = p.label.as_mut() {
            fix(l);
        }
    };
    for s in &mut b.shapes {
        match s {
            w::Shape::Dot(p) => point(p),
            w::Shape::Line(l) => {
                point(&mut l.from);
                point(&mut l.to);
            }
            w::Shape::Pair(p) => {
                if let Some(l) = p.f1.as_mut() {
                    fix(l);
                }
            }
            w::Shape::Empty => {}
        }
    }
    point(&mut b.tagged.point);
    for m in &mut b.maybe {
        if let Some(p) = m.value.as_mut() {
            point(p);
        }
    }
}

#[test]
fn random_wire_messages_survive_wire_native_wire() {
    let mut rng = Rng(0x05ee_db1d);
    let t = w::Tables::new();
    let mut kinds = [0usize; 4];
    for _ in 0..5_000 {
        let mut m = w::Bag::random(&mut rng, &Bounds::default(), 0);
        well_formed(&mut m);
        for s in &m.shapes {
            kinds[match s {
                w::Shape::Empty => 0,
                w::Shape::Dot(_) => 1,
                w::Shape::Line(_) => 2,
                w::Shape::Pair(_) => 3,
            }] += 1;
        }
        let native = Bag::from_wire(m.clone()).unwrap();
        assert_eq!(native.tagged.cache, 0, "the `default` field");
        let back: w::Bag = native.into_wire().unwrap();
        assert_eq!(dump_bag(&back, &t), dump_bag(&m, &t));
    }
    assert!(kinds.iter().all(|k| *k > 100), "{kinds:?}");
}

#[test]
fn native_values_round_trip_and_native_only_variants_refuse() {
    let p = Point { x: 1.5, y: -0.0, label: Some("é😀".to_owned()) };
    let bag = Bag {
        shapes: vec![
            Shape::Empty,
            Shape::Dot(p.clone()),
            Shape::Line { from: p.clone(), to: Point::default() },
            Shape::Pair(7, None),
            Shape::Pair(8, Some("x".to_owned())),
        ],
        color: Color::Green,
        tagged: Tagged { point: p.clone(), cache: 0 },
        id: bind_native::Id(42),
        pair: (2.5, 9),
        maybe: vec![None, Some(p)],
    };
    let wire: w::Bag = bag.into_wire().unwrap();
    assert!(matches!(&wire.shapes[2], w::Shape::Line(l) if l.from.x == 1.5));
    assert!(matches!(&wire.shapes[4], w::Shape::Pair(x) if x.f0 == 8));
    assert_eq!(wire.id.value, 42);
    assert_eq!(Bag::from_wire(wire).unwrap(), bag);
    let mut only = bag.clone();
    only.shapes.push(Shape::Only);
    let r: Result<w::Bag, BindError> = only.into_wire();
    assert_eq!(r.unwrap_err(), BindError::NativeOnly("Shape::Only"));
    // an `into_only` type
    let rep: w::Report = Report { total: 3, colors: vec![Color::Red] }.into_wire().unwrap();
    assert_eq!(rep.colors, vec![w::Color::Red]);
}
