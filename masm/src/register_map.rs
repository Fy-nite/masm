use std::collections::HashMap;

pub struct RegisterMap;

impl RegisterMap {
    pub fn build_name_to_id() -> HashMap<String, u16> {
        let mut m: HashMap<String, u16> = HashMap::new();
        macro_rules! add { ($name:expr, $id:expr) => { m.insert($name.to_string(), $id); }; }
        add!("RAX", 1); add!("RBX", 2); add!("RCX", 3); add!("RDX", 4);
        add!("RSI", 5); add!("RDI", 6); add!("RBP", 7); add!("RSP", 8);
        add!("RIP", 9);
        for i in 0..=15 { add!(&format!("R{}", i), 32 + i as u16); }
        add!("ZF", 100); add!("SF", 101); add!("OF", 102);
        for i in 0..=15 { add!(&format!("FPR{}", i), 200 + i as u16); }
        m
    }

    pub fn build_id_to_name() -> HashMap<u16, String> {
        let m = Self::build_name_to_id();
        let mut r = HashMap::new();
        for (k, v) in m { r.insert(v, k); }
        r
    }
}
