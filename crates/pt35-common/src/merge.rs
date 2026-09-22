use toml::Value;

/// Deep-merge `over` into `base`.
///
/// Tables merge key by key; every other value (including arrays) is replaced
/// wholesale. That is deliberate: a user who redefines `menu.main.entries`
/// means *these* entries, not "append to the defaults".
pub fn merge_toml(base: &mut Value, over: Value) {
    match (base, over) {
        (Value::Table(base), Value::Table(over)) => {
            for (key, value) in over {
                match base.get_mut(&key) {
                    Some(slot) => merge_toml(slot, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (slot, value) => *slot = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Value {
        s.parse().unwrap()
    }

    #[test]
    fn tables_merge_key_by_key() {
        let mut base = v("[color]\nfg = \"white\"\nbg = \"black\"\n");
        merge_toml(&mut base, v("[color]\nbg = \"navy\"\n"));
        assert_eq!(base["color"]["fg"].as_str(), Some("white"));
        assert_eq!(base["color"]["bg"].as_str(), Some("navy"));
    }

    #[test]
    fn arrays_are_replaced_not_appended() {
        let mut base = v("items = [1, 2, 3]\n");
        merge_toml(&mut base, v("items = [9]\n"));
        assert_eq!(base["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn new_keys_are_added() {
        let mut base = v("[menu.main]\ntitle = \"PT35\"\n");
        merge_toml(&mut base, v("[menu.extra]\ntitle = \"Mine\"\n"));
        assert!(base["menu"]["main"].get("title").is_some());
        assert_eq!(base["menu"]["extra"]["title"].as_str(), Some("Mine"));
    }
}
