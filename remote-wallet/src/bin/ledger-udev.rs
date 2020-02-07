/// Implements udev rules on Linux for supported Ledger devices
/// This script must be run with sudo privileges
use std::{
    error,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::Command,
};

const LEDGER_UDEV_RULES: &str = r#"# Nano S
SUBSYSTEMS=="usb", ATTRS{idVendor}=="2c97", ATTRS{idProduct}=="0001|1000|1001|1002|1003|1004|1005|1006|1007|1008|1009|100a|100b|100c|100d|100e|100f|1010|1011|1012|1013|1014|1015|1016|1017|1018|1019|101a|101b|101c|101d|101e|101f", TAG+="uaccess", TAG+="udev-acl", MODE="0666"
# Nano X
SUBSYSTEMS=="usb", ATTRS{idVendor}=="2c97", ATTRS{idProduct}=="0004|4000|4001|4002|4003|4004|4005|4006|4007|4008|4009|400a|400b|400c|400d|400e|400f|4010|4011|4012|4013|4014|4015|4016|4017|4018|4019|401a|401b|401c|401d|401e|401f", TAG+="uaccess", TAG+="udev-acl", MODE="0666""#;

const LEDGER_UDEV_RULES_LOCATION: &str = "/etc/udev/rules.d/20-hw1.rules";

fn main() -> Result<(), Box<dyn error::Error>> {
    if cfg!(target_os = "linux") {
        let mut contents = String::new();
        if Path::new("/etc/udev/rules.d/20-hw1.rules").exists() {
            let mut file = File::open(LEDGER_UDEV_RULES_LOCATION)?;
            file.read_to_string(&mut contents)?;
        }
        if !contents.contains(LEDGER_UDEV_RULES) {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(LEDGER_UDEV_RULES_LOCATION)
                .map_err(|e| {
                    println!("Could not write to file; this script requires sudo privileges");
                    e
                })?;
            file.write_all(LEDGER_UDEV_RULES.as_bytes())?;

            Command::new("udevadm").arg("trigger").output().unwrap();

            Command::new("udevadm")
                .args(&["control", "--reload-rules"])
                .output()
                .unwrap();

            println!("Ledger udev rules written");
        } else {
            println!("Ledger udev rules already in place");
        }
    } else {
        println!("Mismatched target_os; udev rules only required on linux os");
    }
    Ok(())
}
