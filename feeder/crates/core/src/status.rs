//! Machine-readable snapshots of the state shared by GUI and CLI.

use crate::{Engine, logging::escape_json};

fn number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".to_owned()
    }
}

fn numbers<const N: usize>(values: [f64; N]) -> String {
    format!("[{}]", values.map(number).join(","))
}

impl Engine {
    /// JSON snapshot of commanded state, not an OpenVR measurement.
    /// Non-finite numeric values are represented by JSON null.
    pub fn status_json(&self) -> String {
        let world = self.world();
        let devices = world.device_ids().filter_map(|id| {
            let pose = world.pose(id)?;
            let info = self.driver_devices().iter().find(|d| d.device_id == id);
            let serial = info.map_or_else(|| "null".to_owned(), |d| format!("\"{}\"", escape_json(&d.serial)));
            let class = info.map_or_else(|| "null".to_owned(), |d| (d.device_class as u8).to_string());
            let inputs = world.inputs.iter().filter(|((device, _), _)| *device == id)
                .map(|((_, component), value)| format!("{{\"component_id\":{component},\"value\":{}}}", number(f64::from(*value))))
                .collect::<Vec<_>>().join(",");
            Some(format!(
                "{{\"id\":{id},\"serial\":{serial},\"class\":{class},\"pose_valid\":{},\"connected\":{},\"tracking_result\":{},\"position_m\":{},\"rotation_wxyz\":{},\"linear_velocity_m_s\":{},\"angular_velocity_rad_s\":{},\"inputs\":[{inputs}]}}",
                pose.flags.pose_valid(), pose.flags.connected(), pose.tracking_result as u8,
                numbers(pose.position), numbers(pose.rotation), numbers(pose.linear_velocity), numbers(pose.angular_velocity),
            ))
        }).collect::<Vec<_>>().join(",");
        let display = &world.display;
        format!(
            "{{\"schema_version\":1,\"connected\":{},\"devices\":[{devices}],\"display\":{{\"ipd_m\":{},\"projection_left\":{},\"projection_right\":{},\"swap_eyes\":{}}},\"interaction\":{}}}",
            self.connected(),
            number(f64::from(display.ipd_m)),
            numbers(display.projection_left.map(f64::from)),
            numbers(display.projection_right.map(f64::from)),
            display.swap_eyes,
            self.interaction_json(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Config};

    #[test]
    fn snapshot_reflects_all_commanded_state() {
        let mut engine = Engine::new(Config::default());
        engine
            .execute_line("pos trk5 1 2 3")
            .expect("extra tracker");
        engine
            .execute_line("input ctrl-l trigger/value 0.75")
            .expect("input");
        engine.execute_line("swap 1").expect("display");
        let json = engine.status_json();
        assert!(json.contains("\"id\":8"));
        assert!(json.contains("\"position_m\":[1,2,3]"));
        assert!(json.contains("\"component_id\":5,\"value\":0.75"));
        assert!(json.contains("\"swap_eyes\":true"));
        assert_eq!(
            Command::parse("status --json").expect("parse"),
            Command::StatusJson
        );
        assert!(Command::parse("status typo").is_err());
        engine
            .execute_line("pos hmd NaN inf -inf")
            .expect("legacy command permits non-finite values");
        assert!(
            engine
                .status_json()
                .contains("\"position_m\":[null,null,null]")
        );
    }
}
