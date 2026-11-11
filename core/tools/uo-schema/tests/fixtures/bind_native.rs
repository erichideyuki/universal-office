//! The native types of the bind emitter's fixture (schemas/bind_test.uos, schemas/bind_test.bind).
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tagged {
    pub point: Point,
    /// Rust only: not on the wire
    pub cache: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Empty,
    Dot(Point),
    Line {
        from: Point,
        to: Point,
    },
    Pair(u32, Option<String>),
    /// produced by the engine only
    Only,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Green,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Id(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub struct Bag {
    pub shapes: Vec<Shape>,
    pub color: Color,
    pub tagged: Tagged,
    pub id: Id,
    pub pair: (f64, u32),
    pub maybe: Vec<Option<Point>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub total: u32,
    pub colors: Vec<Color>,
}
