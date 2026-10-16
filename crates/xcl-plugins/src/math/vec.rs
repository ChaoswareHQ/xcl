//! Vector types: Vec2, Vec3, Vec4 backed by `glam`.
//!
//! Vectors are stored as typed maps with subtype constants and components as fields.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};

const SUBTYPE_VEC2: u16 = 100;
const SUBTYPE_VEC3: u16 = 101;
const SUBTYPE_VEC4: u16 = 102;

fn f(v: f32) -> f64 { v as f64 }
fn t(v: f64) -> f32 { v as f32 }

fn vec2_map(v: glam::Vec2) -> Value {
    let mut m = HashMap::with_capacity(3);
    m.insert("x".into(), Value::from_f64(f(v.x)));
    m.insert("y".into(), Value::from_f64(f(v.y)));
    Value::from_map_boxed_typed_v(m, SUBTYPE_VEC2, 0)
}

fn vec3_map(v: glam::Vec3) -> Value {
    let mut m = HashMap::with_capacity(4);
    m.insert("x".into(), Value::from_f64(f(v.x)));
    m.insert("y".into(), Value::from_f64(f(v.y)));
    m.insert("z".into(), Value::from_f64(f(v.z)));
    Value::from_map_boxed_typed_v(m, SUBTYPE_VEC3, 0)
}

fn vec4_map(v: glam::Vec4) -> Value {
    let mut m = HashMap::with_capacity(5);
    m.insert("x".into(), Value::from_f64(f(v.x)));
    m.insert("y".into(), Value::from_f64(f(v.y)));
    m.insert("z".into(), Value::from_f64(f(v.z)));
    m.insert("w".into(), Value::from_f64(f(v.w)));
    Value::from_map_boxed_typed_v(m, SUBTYPE_VEC4, 0)
}

fn gc(v: Value, name: &str) -> f32 {
    t(v.as_map().and_then(|m| m.get(name)).map(|val| val.as_float()).unwrap_or(0.0))
}

fn v2(v: Value) -> glam::Vec2 { glam::Vec2::new(gc(v,"x"), gc(v,"y")) }
fn v3(v: Value) -> glam::Vec3 { glam::Vec3::new(gc(v,"x"), gc(v,"y"), gc(v,"z")) }
fn v4(v: Value) -> glam::Vec4 { glam::Vec4::new(gc(v,"x"), gc(v,"y"), gc(v,"z"), gc(v,"w")) }

// ─── Constructors ───

#[xcl_fn(name = "math.vec2", args = "x: float; y: float", ret = "vec2")]
fn math_vec2(args: &[Value]) -> Value {
    vec2_map(glam::Vec2::new(t(args[0].as_float()), t(args[1].as_float())))
}

#[xcl_fn(name = "math.vec3", args = "x: float; y: float; z: float", ret = "vec3")]
fn math_vec3(args: &[Value]) -> Value {
    vec3_map(glam::Vec3::new(t(args[0].as_float()), t(args[1].as_float()), t(args[2].as_float())))
}

#[xcl_fn(name = "math.vec4", args = "x: float; y: float; z: float; w: float", ret = "vec4")]
fn math_vec4(args: &[Value]) -> Value {
    vec4_map(glam::Vec4::new(t(args[0].as_float()), t(args[1].as_float()), t(args[2].as_float()), t(args[3].as_float())))
}

// ─── Properties ───

#[xcl_fn(name = "vec.len", args = "v: vec", ret = "float")]
fn vec_len(args: &[Value]) -> Value {
    let v = args[0];
    let st = v.map_subtype().unwrap_or(0);
    let len = match st {
        SUBTYPE_VEC2 => f(v2(v).length()),
        SUBTYPE_VEC3 => f(v3(v).length()),
        SUBTYPE_VEC4 => f(v4(v).length()),
        _ => 0.0,
    };
    Value::from_f64(len)
}

#[xcl_fn(name = "vec.len_sq", args = "v: vec", ret = "float")]
fn vec_len_sq(args: &[Value]) -> Value {
    let v = args[0];
    let st = v.map_subtype().unwrap_or(0);
    let len_sq = match st {
        SUBTYPE_VEC2 => f(v2(v).length_squared()),
        SUBTYPE_VEC3 => f(v3(v).length_squared()),
        SUBTYPE_VEC4 => f(v4(v).length_squared()),
        _ => 0.0,
    };
    Value::from_f64(len_sq)
}

// ─── Arithmetic ───

#[xcl_fn(name = "vec.add", args = "a: vec; b: vec", ret = "vec")]
fn vec_add(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]) + v2(args[1])),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]) + v3(args[1])),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]) + v4(args[1])),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.sub", args = "a: vec; b: vec", ret = "vec")]
fn vec_sub(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]) - v2(args[1])),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]) - v3(args[1])),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]) - v4(args[1])),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.mul", args = "a: vec; b: vec", ret = "vec")]
fn vec_mul(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]) * v2(args[1])),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]) * v3(args[1])),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]) * v4(args[1])),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.mul_scalar", args = "v: vec; s: float", ret = "vec")]
fn vec_mul_scalar(args: &[Value]) -> Value {
    let s = t(args[1].as_float());
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]) * s),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]) * s),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]) * s),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.div_scalar", args = "v: vec; s: float", ret = "vec")]
fn vec_div_scalar(args: &[Value]) -> Value {
    let s = t(args[1].as_float());
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]) / s),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]) / s),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]) / s),
        _ => Value::nil(),
    }
}

// ─── Products ───

#[xcl_fn(name = "vec.dot", args = "a: vec; b: vec", ret = "float")]
fn vec_dot(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    let dot = match st {
        SUBTYPE_VEC2 => f(v2(args[0]).dot(v2(args[1]))),
        SUBTYPE_VEC3 => f(v3(args[0]).dot(v3(args[1]))),
        SUBTYPE_VEC4 => f(v4(args[0]).dot(v4(args[1]))),
        _ => 0.0,
    };
    Value::from_f64(dot)
}

#[xcl_fn(name = "vec.cross", args = "a: vec3; b: vec3", ret = "vec3")]
fn vec_cross(args: &[Value]) -> Value {
    vec3_map(v3(args[0]).cross(v3(args[1])))
}

// ─── Operations ───

#[xcl_fn(name = "vec.normalize", args = "v: vec", ret = "vec")]
fn vec_normalize(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).normalize()),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).normalize()),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).normalize()),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.lerp", args = "a: vec; b: vec; t: float", ret = "vec")]
fn vec_lerp(args: &[Value]) -> Value {
    let t = t(args[2].as_float());
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).lerp(v2(args[1]), t)),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).lerp(v3(args[1]), t)),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).lerp(v4(args[1]), t)),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.angle_to", args = "a: vec; b: vec", ret = "float")]
fn vec_angle_to(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    let angle = match st {
        SUBTYPE_VEC2 => f(v2(args[0]).angle_to(v2(args[1]))),
        SUBTYPE_VEC3 => {
            let a = v3(args[0]); let b = v3(args[1]);
            f((a.dot(b) / (a.length() * b.length())).acos())
        }
        _ => 0.0,
    };
    Value::from_f64(angle)
}

#[xcl_fn(name = "vec.distance_to", args = "a: vec; b: vec", ret = "float")]
fn vec_distance_to(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    let dist = match st {
        SUBTYPE_VEC2 => f(v2(args[0]).distance(v2(args[1]))),
        SUBTYPE_VEC3 => f(v3(args[0]).distance(v3(args[1]))),
        SUBTYPE_VEC4 => f(v4(args[0]).distance(v4(args[1]))),
        _ => 0.0,
    };
    Value::from_f64(dist)
}

#[xcl_fn(name = "vec.reflect", args = "v: vec; normal: vec", ret = "vec")]
fn vec_reflect(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).reflect(v2(args[1]).normalize())),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).reflect(v3(args[1]).normalize())),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.project_on", args = "a: vec; b: vec", ret = "vec")]
fn vec_project_on(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => {
            let va = v2(args[0]); let vb = v2(args[1]);
            vec2_map(vb * (va.dot(vb) / vb.dot(vb)))
        }
        SUBTYPE_VEC3 => {
            let va = v3(args[0]); let vb = v3(args[1]);
            vec3_map(vb * (va.dot(vb) / vb.dot(vb)))
        }
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.clamp_len", args = "v: vec; max: float", ret = "vec")]
fn vec_clamp_len(args: &[Value]) -> Value {
    let max = t(args[1].as_float());
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).clamp_length_max(max)),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).clamp_length_max(max)),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).clamp_length_max(max)),
        _ => Value::nil(),
    }
}

// ─── Component-wise ───

#[xcl_fn(name = "vec.abs", args = "v: vec", ret = "vec")]
fn vec_abs(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).abs()),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).abs()),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).abs()),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.floor", args = "v: vec", ret = "vec")]
fn vec_floor(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).floor()),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).floor()),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).floor()),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.ceil", args = "v: vec", ret = "vec")]
fn vec_ceil(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).ceil()),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).ceil()),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).ceil()),
        _ => Value::nil(),
    }
}

#[xcl_fn(name = "vec.round", args = "v: vec", ret = "vec")]
fn vec_round(args: &[Value]) -> Value {
    let st = args[0].map_subtype().unwrap_or(0);
    match st {
        SUBTYPE_VEC2 => vec2_map(v2(args[0]).round()),
        SUBTYPE_VEC3 => vec3_map(v3(args[0]).round()),
        SUBTYPE_VEC4 => vec4_map(v4(args[0]).round()),
        _ => Value::nil(),
    }
}

// ─── String representation ───

#[xcl_fn(name = "vec.to_string", args = "v: vec", ret = "str")]
fn vec_to_string(ctx: &mut DomainCtx, args: &[Value]) -> Value {
    let v = args[0];
    let st = v.map_subtype().unwrap_or(0);
    let s = match st {
        SUBTYPE_VEC2 => format!("Vec2({}, {})", gc(v,"x"), gc(v,"y")),
        SUBTYPE_VEC3 => format!("Vec3({}, {}, {})", gc(v,"x"), gc(v,"y"), gc(v,"z")),
        SUBTYPE_VEC4 => format!("Vec4({}, {}, {}, {})", gc(v,"x"), gc(v,"y"), gc(v,"z"), gc(v,"w")),
        _ => "Vec(?)".to_string(),
    };
    ctx.string(s)
}
