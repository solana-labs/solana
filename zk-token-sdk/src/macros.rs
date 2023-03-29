// Internal macros, used to defined the 'main' impls
#[macro_export]
macro_rules! define_add_variants {
    (LHS = $lhs:ty, RHS = $rhs:ty, Output = $out:ty) => {
        impl<'b> Add<&'b $rhs> for $lhs {
            type Output = $out;
            fn add(self, rhs: &'b $rhs) -> $out {
                &self + rhs
            }
        }

        impl<'a> Add<$rhs> for &'a $lhs {
            type Output = $out;
            fn add(self, rhs: $rhs) -> $out {
                self + &rhs
            }
        }

        impl Add<$rhs> for $lhs {
            type Output = $out;
            fn add(self, rhs: $rhs) -> $out {
                &self + &rhs
            }
        }
    };
}

macro_rules! define_sub_variants {
    (LHS = $lhs:ty, RHS = $rhs:ty, Output = $out:ty) => {
        impl<'b> Sub<&'b $rhs> for $lhs {
            type Output = $out;
            fn sub(self, rhs: &'b $rhs) -> $out {
                &self - rhs
            }
        }

        impl<'a> Sub<$rhs> for &'a $lhs {
            type Output = $out;
            fn sub(self, rhs: $rhs) -> $out {
                self - &rhs
            }
        }

        impl Sub<$rhs> for $lhs {
            type Output = $out;
            fn sub(self, rhs: $rhs) -> $out {
                &self - &rhs
            }
        }
    };
}

macro_rules! define_mul_variants {
    (LHS = $lhs:ty, RHS = $rhs:ty, Output = $out:ty) => {
        impl<'b> Mul<&'b $rhs> for $lhs {
            type Output = $out;
            fn mul(self, rhs: &'b $rhs) -> $out {
                &self * rhs
            }
        }

        impl<'a> Mul<$rhs> for &'a $lhs {
            type Output = $out;
            fn mul(self, rhs: $rhs) -> $out {
                self * &rhs
            }
        }

        impl Mul<$rhs> for $lhs {
            type Output = $out;
            fn mul(self, rhs: $rhs) -> $out {
                &self * &rhs
            }
        }
    };
}

macro_rules! impl_from_transcript_error {
    ($sigma_error_type:ty) => {
        impl From<TranscriptError> for $sigma_error_type {
            fn from(err: TranscriptError) -> Self {
                ProofVerificationError::Transcript(err).into()
            }
        }
    };
}
