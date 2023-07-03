#[derive(Debug)]
pub struct UnmarshalError {}

impl UnmarshalError {
    fn new() -> UnmarshalError {
        return UnmarshalError {};
    }
}

pub trait Unmarshal {
    fn unmarshal(b: &[u8]) -> std::result::Result<(), UnmarshalError>;
    fn size() -> i32;
}

#[derive(Debug)]
pub struct Point {
    x: f64,
    y: f64,
    z: f64,
}

impl Point {
    pub fn new(x: f64, y: f64, z: f64) -> Point {
        Point { x, y, z }
    }
}

#[derive(Debug)]
pub struct Q3D {
    p: Point,
}

impl Q3D {
    pub fn new(x: f64, y: f64, z: f64) -> Q3D {
        Q3D {
            p: Point::new(x, y, z),
        }
    }
}

impl Unmarshal for Q3D {
    fn unmarshal(b: &[u8]) -> std::result::Result<(), UnmarshalError> {
        if b.len() == 0 {}
        return Err(UnmarshalError::new());
    }

    fn size() -> i32 {
        return 4;
    }
}
