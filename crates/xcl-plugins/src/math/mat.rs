//! Matrix types: Mat2, Mat3, Mat4 backed by `glam`.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

const SUBTYPE_MAT2: u16 = 110;
const SUBTYPE_MAT3: u16 = 111;
const SUBTYPE_MAT4: u16 = 112;

fn f(v: f32) -> f64 { v as f64 }
fn t(v: f64) -> f32 { v as f32 }

fn mat2_map(m: glam::Mat2) -> Value {
    let mut map = HashMap::with_capacity(6);
    map.insert("rows".into(), Value::from_i48(2));
    map.insert("cols".into(), Value::from_i48(2));
    for r in 0..2 {
        for c in 0..2 {
            map.insert(format!("m{}{}", r, c), Value::from_f64(f(m.col(c)[r])));
        }
    }
    Value::from_map_boxed_typed_v(map, SUBTYPE_MAT2, 0)
}

fn mat3_map(m: glam::Mat3) -> Value {
    let mut map = HashMap::with_capacity(12);
    map.insert("rows".into(), Value::from_i48(3));
    map.insert("cols".into(), Value::from_i48(3));
    for r in 0..3 {
        for c in 0..3 {
            map.insert(format!("m{}{}", r, c), Value::from_f64(f(m.col(c)[r])));
        }
    }
    Value::from_map_boxed_typed_v(map, SUBTYPE_MAT3, 0)
}

fn mat4_map(m: glam::Mat4) -> Value {
    let mut map = HashMap::with_capacity(20);
    map.insert("rows".into(), Value::from_i48(4));
    map.insert("cols".into(), Value::from_i48(4));
    for r in 0..4 {
        for c in 0..4 {
            map.insert(format!("m{}{}", r, c), Value::from_f64(f(m.col(c)[r])));
        }
    }
    Value::from_map_boxed_typed_v(map, SUBTYPE_MAT4, 0)
}

fn ge(m: Value, r: usize, c: usize) -> f32 {
    t(m.as_map().and_then(|map| map.get(&format!("m{}{}", r, c))).map(|v| v.as_float()).unwrap_or(0.0))
}

fn m2(v: Value) -> glam::Mat2 {
    glam::Mat2::from_cols_array(&[ge(v,0,0), ge(v,1,0), ge(v,0,1), ge(v,1,1)])
}
fn m3(v: Value) -> glam::Mat3 {
    glam::Mat3::from_cols_array(&[
        ge(v,0,0), ge(v,1,0), ge(v,2,0),
        ge(v,0,1), ge(v,1,1), ge(v,2,1),
        ge(v,0,2), ge(v,1,2), ge(v,2,2),
    ])
}
fn m4(v: Value) -> glam::Mat4 {
    let mut cols = [0.0f32; 16];
    for c in 0..4 { for r in 0..4 { cols[c*4+r] = ge(v, r, c); } }
    glam::Mat4::from_cols_array(&cols)
}

// ─── Constructors ───

#[xcl_fn]
fn math_mat2_identity() -> Value { mat2_map(glam::Mat2::IDENTITY) }

#[xcl_fn]
fn math_mat3_identity() -> Value { mat3_map(glam::Mat3::IDENTITY) }

#[xcl_fn]
fn math_mat4_identity() -> Value { mat4_map(glam::Mat4::IDENTITY) }

#[xcl_fn]
fn math_mat4_from_cols(args: &[Value]) -> Value {
    let a: &[f64] = &args[..].iter().map(|v| v.as_float()).collect::<Vec<_>>()[..];
    let c0 = glam::Vec4::new(t(a[0]), t(a[1]), t(a[2]), t(a[3]));
    let c1 = glam::Vec4::new(t(a[4]), t(a[5]), t(a[6]), t(a[7]));
    let c2 = glam::Vec4::new(t(a[8]), t(a[9]), t(a[10]), t(a[11]));
    let c3 = glam::Vec4::new(t(a[12]), t(a[13]), t(a[14]), t(a[15]));
    mat4_map(glam::Mat4::from_cols(c0, c1, c2, c3))
}

// ─── Properties ───

#[xcl_fn]
fn mat_det(args: &[Value]) -> Value {
    let m = args[0];
    let st = m.map_subtype().unwrap_or(0);
    let det = match st {
        SUBTYPE_MAT2 => f(m2(m).determinant()),
        SUBTYPE_MAT3 => f(m3(m).determinant()),
        SUBTYPE_MAT4 => f(m4(m).determinant()),
        _ => 0.0,
    };
    Value::from_f64(det)
}

#[xcl_fn]
fn mat_trace(args: &[Value]) -> Value {
    let m = args[0];
    let st = m.map_subtype().unwrap_or(0);
    let trace = match st {
        SUBTYPE_MAT2 => f(ge(m,0,0) + ge(m,1,1)),
        SUBTYPE_MAT3 => f(ge(m,0,0) + ge(m,1,1) + ge(m,2,2)),
        SUBTYPE_MAT4 => f(ge(m,0,0) + ge(m,1,1) + ge(m,2,2) + ge(m,3,3)),
        _ => 0.0,
    };
    Value::from_f64(trace)
}

// ─── Operations ───

#[xcl_fn]
fn mat_mul(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_MAT2 => mat2_map(m2(args[0]) * m2(args[1])),
        SUBTYPE_MAT3 => mat3_map(m3(args[0]) * m3(args[1])),
        SUBTYPE_MAT4 => mat4_map(m4(args[0]) * m4(args[1])),
        _ => Value::nil(),
    }
}

#[xcl_fn]
fn mat_transpose(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_MAT2 => mat2_map(m2(args[0]).transpose()),
        SUBTYPE_MAT3 => mat3_map(m3(args[0]).transpose()),
        SUBTYPE_MAT4 => mat4_map(m4(args[0]).transpose()),
        _ => Value::nil(),
    }
}

#[xcl_fn]
fn mat_inv(args: &[Value]) -> Value {
    let m = args[0];
    let st = m.map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_MAT2 => {
            let inv = m2(m).inverse();
            mat2_map(inv)
        }
        SUBTYPE_MAT3 => {
            let inv = m3(m).inverse();
            mat3_map(inv)
        }
        SUBTYPE_MAT4 => {
            let inv = m4(m).inverse();
            mat4_map(inv)
        }
        _ => Value::nil(),
    }
}

#[xcl_fn]
fn mat_mul_scalar(args: &[Value]) -> Value {
    let s = t(args[1].as_float());
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_MAT2 => mat2_map(m2(args[0]) * s),
        SUBTYPE_MAT3 => mat3_map(m3(args[0]) * s),
        SUBTYPE_MAT4 => mat4_map(m4(args[0]) * s),
        _ => Value::nil(),
    }
}

// ─── Transform ───

#[xcl_fn]
fn mat_transform_vec3(args: &[Value]) -> Value {
    let gm = m4(args[0]);
    let gv = glam::Vec3::new(gc_v(args[1],"x"), gc_v(args[1],"y"), gc_v(args[1],"z"));
    let result = gm.transform_point3(gv);
    let mut map = HashMap::with_capacity(4);
    map.insert("x".into(), Value::from_f64(f(result.x)));
    map.insert("y".into(), Value::from_f64(f(result.y)));
    map.insert("z".into(), Value::from_f64(f(result.z)));
    Value::from_map_boxed_typed_v(map, 101, 0)
}

#[xcl_fn]
fn mat_transform_vec4(args: &[Value]) -> Value {
    let gm = m4(args[0]);
    let gv = glam::Vec4::new(gc_v(args[1],"x"), gc_v(args[1],"y"), gc_v(args[1],"z"), gc_v(args[1],"w"));
    let result = gm.mul_vec4(gv);
    let mut map = HashMap::with_capacity(5);
    map.insert("x".into(), Value::from_f64(f(result.x)));
    map.insert("y".into(), Value::from_f64(f(result.y)));
    map.insert("z".into(), Value::from_f64(f(result.z)));
    map.insert("w".into(), Value::from_f64(f(result.w)));
    Value::from_map_boxed_typed_v(map, 102, 0)
}

fn gc_v(v: Value, name: &str) -> f32 {
    t(v.as_map().and_then(|map| map.get(name)).map(|val| val.as_float()).unwrap_or(0.0))
}

// ─── Camera helpers ───

#[xcl_fn]
fn math_mat4_perspective_fov(args: &[Value]) -> Value {
    let fov_y = t(args[0].as_float());
    let aspect = t(args[1].as_float());
    let near = t(args[2].as_float());
    let far = t(args[3].as_float());
    let cot = 1.0 / (fov_y * 0.5).tan();
    let mut cols = [0.0f32; 16];
    cols[0] = cot / aspect; cols[5] = cot;
    cols[10] = (far + near) / (far - near);
    cols[11] = -1.0;
    cols[14] = (2.0 * far * near) / (near - far);
    mat4_map(glam::Mat4::from_cols_array(&cols))
}

#[xcl_fn]
fn math_mat4_look_at(args: &[Value]) -> Value {
    let eye = glam::Vec3::new(t(args[0].as_float()), t(args[1].as_float()), t(args[2].as_float()));
    let target = glam::Vec3::new(t(args[3].as_float()), t(args[4].as_float()), t(args[5].as_float()));
    let up = glam::Vec3::new(t(args[6].as_float()), t(args[7].as_float()), t(args[8].as_float()));
    let z = (eye - target).normalize();
    let x = up.cross(z).normalize();
    let y = z.cross(x);
    let mut cols = [0.0f32; 16];
    cols[0] = x.x; cols[1] = x.y; cols[2] = x.z;
    cols[4] = y.x; cols[5] = y.y; cols[6] = y.z;
    cols[8] = z.x; cols[9] = z.y; cols[10] = z.z;
    cols[12] = -x.dot(eye); cols[13] = -y.dot(eye); cols[14] = -z.dot(eye);
    cols[15] = 1.0;
    mat4_map(glam::Mat4::from_cols_array(&cols))
}
