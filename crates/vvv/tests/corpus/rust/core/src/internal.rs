pub struct Secret;

impl Secret {
    pub fn reveal(&self) -> &'static str {
        "shh"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reveals() {
        assert_eq!(Secret.reveal(), "shh");
    }
}
