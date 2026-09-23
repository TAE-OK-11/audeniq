use std::{env, fs, path::PathBuf};
fn main() {
    let path = "../../config/states.json";
    println!("cargo:rerun-if-changed={path}");
    let c: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let mut output = String::new();
    for (axis, values) in c["axes"].as_object().unwrap() {
        let name = axis
            .split('_')
            .map(|s| {
                let mut c = s.chars();
                c.next().unwrap().to_uppercase().collect::<String>() + c.as_str()
            })
            .collect::<String>();
        output += &format!(
            "#[allow(non_camel_case_types)] #[derive(Debug,Clone,Copy,PartialEq,Eq,serde::Serialize,serde::Deserialize)] pub enum {name} {{"
        );
        for value in values.as_array().unwrap() {
            output += value.as_str().unwrap();
            output += ",";
        }
        output += "}\n";
        output += &format!(
            "impl {name} {{pub fn transition(self,next:Self)->crate::error::Result<Self>{{let a=serde_json::to_value(self).unwrap();let b=serde_json::to_value(next).unwrap();crate::domain::transition(\"{axis}\",a.as_str().unwrap(),b.as_str().unwrap())?;Ok(next)}}}}\n"
        );
    }
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("states.rs"),
        output,
    )
    .unwrap();
}
