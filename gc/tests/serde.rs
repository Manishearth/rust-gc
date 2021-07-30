#![cfg(feature = "serde")]

use gc::Gc;
use serde_json::json;
use std::collections::HashMap;

type Example = Gc<HashMap<String, Gc<Vec<i32>>>>;

#[test]
fn serde_tests() {
    let value = json!({
        "hello": [104, 101, 108, 108, 111],
        "world": [119, 111, 114, 108, 100],
    });

    let mut expected = HashMap::new();
    expected.insert("hello".to_string(), Gc::new(vec![104, 101, 108, 108, 111]));
    expected.insert("world".to_string(), Gc::new(vec![119, 111, 114, 108, 100]));
    let expected = Gc::new(expected);

    assert_eq!(serde_json::to_value(&expected).unwrap(), value);
    assert_eq!(serde_json::from_value::<Example>(value).unwrap(), expected);
}
