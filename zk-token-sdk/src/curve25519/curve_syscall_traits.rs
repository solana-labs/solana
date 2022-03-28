
pub trait PointValidation {
    type Point;

    fn validate_point(&self) -> bool;
}

pub trait GroupOperations {
    type Point;
    type Scalar;

    fn add(left_point: &Self::Point, right_point: &Self::Point) -> Option<Self::Point>;
    fn subtract(left_point: &Self::Point, right_point: &Self::Point) -> Option<Self::Point>;
    fn multiply(scalar: &Self::Scalar, point: &Self::Point) -> Option<Self::Point>;
}

pub trait MultiScalarMultiplication {
    type Scalar;
    type Point;

    fn multiscalar_multiply(scalars: Vec<&Self::Scalar>, points: Vec<&Self::Point>) -> Option<Self::Point>;
}

pub trait Pairing {
    type G1Point;
    type G2Point;
    type GTPoint;

    fn pairing_map(left_point: &Self::G1Point, right_point: &Self::G2Point) -> Option<Self::GTPoint>;
}
