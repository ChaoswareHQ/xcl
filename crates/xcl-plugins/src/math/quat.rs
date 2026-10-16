//! Quaternion type for 3D rotations backed by `glam`.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

const SUBTYPE_QUAT: u16 = 120;

fn f(v: f32) -> f64 { v as f64 }
fn t(v: f64) -> f32 { v as f32 }

fn quat_map(q: glam::Quat) -> Value {
    let mut m = HashMap::with_capacity(5);
    m.insert("w".into(), Value::from_f64(f(q.w)));
    m.insert("x".into(), Value::from_f64(f(q.x)));
    m.insert("y".into(), Value::from_f64(f(q.y)));
    m.insert("z".into(), Value::from_f64(f(q.z)));
    Value::from_map_boxed_typed_v(m, SUBTYPE_QUAT, 0)
}

fn gc(v: Value, name: &str) -> f32 {
    t(v.as_map().and_then(|m| m.get(name)).map(|val| val.as_float()).unwrap_or(0.0))
}

fn gq(v: Value) -> glam::Quat {
    glam::Quat::from_xyzw(gc(v,"x"), gc(v,"y"), gc(v,"z"), gc(v,"w"))
}

// ─── Constructors ───

#[xcl_fn(name = "math.quat", args = "w: float; x: float; y: float; z: float", ret = "quat")]
fn math_quat(args: &[Value]) -> Value {
    quat_map(glam::Quat::from_xyzw(t(args[1].as_float()), t(args[2].as_float()), t(args[3].as_float()), t(args[0].as_float())))
}

#[xcl_fn(name = "math.quat_from_angle_axis", args = "angle: float; ax: float; ay: float; az: float", ret = "quat")]
fn math_quat_from_angle_axis(args: &[Value]) -> Value {
    let angle = t(args[0].as_float());
    let axis = glam::Vec3::new(t(args[1].as_float()), t(args[2].as_float()), t(args[3].as_float())).normalize();
    quat_map(glam::Quat::from_axis_angle(axis, angle))
}

#[xcl_fn(name = "math.quat_from_euler", args = "pitch: float; yaw: float; roll: float", ret = "quat")]
fn math_quat_from_euler(args: &[Value]) -> Value {
    let pitch = t(args[0].as_float());
    let yaw = t(args[1].as_float());
    let roll = t(args[2].as_float());
    quat_map(glam::Quat::from_euler(glam::EulerRot::YXZ, yaw, pitch, roll))
}

#[xcl_fn(name = "math.quat_identity", ret = "quat")]
fn math_quat_identity() -> Value {
    quat_map(glam::Quat::IDENTITY)
}

// ─── Operations ───

#[xcl_fn(name = "quat.mul", args = "a: quat; b: quat", ret = "quat")]
fn quat_mul(args: &[Value]) -> Value {
    quat_map(gq(args[0]) * gq(args[1]))
}

#[xcl_fn(name = "quat.conj", args = "q: quat", ret = "quat")]
fn quat_conj(args: &[Value]) -> Value {
    quat_map(gq(args[0]).conjugate())
}

#[xcl_fn(name = "quat.normalize", args = "q: quat", ret = "quat")]
fn quat_normalize(args: &[Value]) -> Value {
    quat_map(gq(args[0]).normalize())
}

#[xcl_fn(name = "quat.inverse", args = "q: quat", ret = "quat")]
fn quat_inverse(args: &[Value]) -> Value {
    quat_map(gq(args[0]).inverse())
}

// ─── Conversions ───

#[xcl_fn(name = "quat.to_matrix", args = "q: quat", ret = "mat4")]
fn quat_to_matrix(args: &[Value]) -> Value {
    let m = glam::Mat4::from_quat(gq(args[0]));
    let mut map = HashMap::with_capacity(20);
    map.insert("rows".into(), Value::from_i48(4));
    map.insert("cols".into(), Value::from_i48(4));
    for r in 0..4 {
        for c in 0..4 {
            map.insert(format!("m{}{}", r, c), Value::from_f64(f(m.col(c)[r])));
        }
    }
    Value::from_map_boxed_typed_v(map, 112, 0)
}

#[xcl_fn(name = "quat.to_euler", args = "q: quat", ret = "vec3")]
fn quat_to_euler(args: &[Value]) -> Value {
    let euler = gq(args[0]).to_euler(glam::EulerRot::YXZ);
    let mut m = HashMap::with_capacity(4);
    m.insert("x".into(), Value::from_f64(f(euler.1)));
    m.insert("y".into(), Value::from_f64(f(euler.0)));
    m.insert("z".into(), Value::from_f64(f(euler.2)));
    Value::from_map_boxed_typed_v(m, 101, 0)
}

// ─── Interpolation ───

#[xcl_fn(name = "quat.slerp", args = "a: quat; b: quat; t: float", ret = "quat")]
fn quat_slerp(args: &[Value]) -> Value {
    let s = t(args[2].as_float());
    quat_map(gq(args[0]).slerp(gq(args[1]), s))
}
