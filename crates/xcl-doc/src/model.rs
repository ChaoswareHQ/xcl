/// The complete documentation model collected from registries and source files.

#[derive(Debug, Clone, serde::Serialize)]
pub struct DocModel {
    pub domains:  Vec<Domain>,
    pub natives:  Vec<Native>,
    pub types:    Vec<TypeDoc>,
    pub traits:   Vec<TraitDoc>,
    pub impls:    Vec<ImplDoc>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Domain {
    pub name:    String,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Native {
    pub full_name: String,               // "io.read" or "print"
    pub domain:    Option<String>,
    pub method:    String,
    pub args:      Option<String>,
    pub ret:       Option<String>,
    pub params:    Vec<ParamDoc>,        // parsed from args: "path: str; mode: str"
    pub named:     Vec<NamedDoc>,
    pub summary:   Option<String>,
    pub arg_desc:  Vec<(String, String)>, // (arg_name, description)
    pub description: Option<String>,     // full multi-paragraph description
    pub examples:  Vec<String>,          // code examples
    pub notes:     Vec<String>,          // additional notes
}

/// A single function parameter parsed from the args string.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParamDoc {
    pub name: String,
    pub ty:   String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NamedDoc {
    pub name:    String,
    pub ty:      String,
    pub default: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TypeDoc {
    pub name:     String,
    pub domain:   Option<String>,
    pub kind:     String,                // "Opaque" | "Record" | "Sum" | "Alias" | "Primitive"
    pub summary:  Option<String>,
    pub description: Option<String>,     // full multi-paragraph description
    pub examples:  Vec<String>,          // code examples
    pub notes:     Vec<String>,          // additional notes
    pub fields:   Vec<FieldDoc>,
    pub variants: Vec<VariantDoc>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldDoc {
    pub name:     String,
    pub ty:       String,
    pub optional: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VariantDoc {
    pub name:   String,
    pub fields: Vec<FieldDoc>,
}

/// An impl block: "Type is Trait" — like `Dog is Animal { bark() }`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImplDoc {
    pub type_name:  String,
    pub trait_name: String,
    pub domain:     Option<String>,
    pub methods:    Vec<ImplMethodDoc>,
}

/// A trait definition: interface with method signatures.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TraitDoc {
    pub name:     String,
    pub domain:   Option<String>,
    pub summary:  Option<String>,
    pub methods:  Vec<TraitMethodDoc>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TraitMethodDoc {
    pub name:    String,
    pub args:    Option<String>,
    pub ret:     Option<String>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImplMethodDoc {
    pub name: String,
    pub args: Option<String>,
    pub ret:  Option<String>,
    pub summary: Option<String>,
}
