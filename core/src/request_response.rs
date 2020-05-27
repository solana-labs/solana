pub trait RequestResponse {
    type Response;
    fn num_expected_responses(&self) -> u32;
    fn verify_response(&self, response: &Self::Response) -> bool;
}
