pub fn recursive() {
    self::recursive();
    self::sibling();
}

pub fn sibling() {}

pub fn outside() { self::recursive(); }
