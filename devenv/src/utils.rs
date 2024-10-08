use serde_json::{Map, Value};

fn filter_attr(json_value: &mut Value, key: &str) {
    match json_value {
        Value::Object(ref mut map) => {
            map.remove(key);

            for val in map.values_mut() {
                filter_attr(val, key);
            }
        }
        Value::Array(ref mut arr) => {
            for val in arr.iter_mut() {
                filter_attr(val, key);
            }
        }
        _ => {}
    }
}

pub fn filter_json(json_value: &mut Value, keys: Vec<&str>) -> Value {
    for key in keys {
        filter_attr(json_value, key)
    }

    json_value.clone()
}

pub fn insert_nested_value(nested_map: &mut Map<String, Value>, loc: &[String], value: Value) {
    let mut current = nested_map;

    for (i, key) in loc.iter().enumerate() {
        if i == loc.len() - 1 {
            current.insert(key.clone(), value.clone());
        } else {
            current = current
                .entry(key.clone())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("Should be an object");
        }
    }
}

pub fn flatten(json_value: Value) -> Value {
    let mut nested_map = Map::new();

    if let Value::Object(flat_map) = json_value {
        for (_, v) in flat_map {
            if let Some(loc_array) = v.get("loc").and_then(|v| v.as_array()) {
                let loc_vec: Vec<String> = loc_array
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                insert_nested_value(&mut nested_map, &loc_vec, v);
            }
        }
    }
    let nested_json = Value::Object(nested_map);
    return nested_json;
}
