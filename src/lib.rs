use std::{collections::HashMap, fmt::Display};

use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};

use syn::{
    Token, braced,
    parse::{Parse, ParseStream},
    parse_quote,
    visit::Visit,
    visit_mut::VisitMut,
};

struct MozartInput {
    function: syn::ItemFn,
    sections: Vec<Section>,
}

struct Section {
    name: syn::Ident,
    variants: Vec<Variant>,
}

struct Variant {
    name: syn::Ident,
    ty: VariantType,
}

enum VariantType {
    Block(syn::Block),
    Expr(syn::Expr),
    Type(syn::Type),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum VariantKind {
    Expr,
    Type,
    Block,
}

impl From<&Variant> for syn::Expr {
    fn from(v: &Variant) -> Self {
        match &v.ty {
            VariantType::Expr(e) => e.clone(),
            VariantType::Block(b) => syn::Expr::Block(syn::ExprBlock {
                attrs: Vec::new(),
                label: None,
                block: b.clone(),
            }),
            VariantType::Type(_) => panic!(),
        }
    }
}

impl From<&Variant> for syn::Type {
    fn from(v: &Variant) -> Self {
        match &v.ty {
            VariantType::Type(t) => t.clone(),
            _ => panic!(),
        }
    }
}

#[proc_macro]
pub fn mozart(input: TokenStream) -> TokenStream {
    let parsed = syn::parse_macro_input!(input as MozartInput);

    match compose(parsed) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

impl Parse for MozartInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Find the function in the macro, and generate mappings
        let function = get_function_from_stream(input)?;
        let mapping = VariantMapping::build(&function)?;

        // Except for the function, go over the macro body, and create sections
        let mut sections = Vec::<Section>::new();
        while !looks_like_fn_item(input) {
            // First get section name
            let section_name: syn::Ident = input.parse()?;

            if sections.iter().any(|section| section.name == section_name) {
                return Err(syn::Error::new_spanned(
                    &section_name,
                    error_msg::DUPLICATE_SECTION,
                ));
            }

            input.parse::<Token![=>]>()?;

            // Extract the content in the section
            let content;
            braced!(content in input);

            if content.is_empty() {
                return Err(content.error(error_msg::EMPTY_SECTION));
            }

            // Get the VariantKind via the mapping
            let kind = match mapping.get(&section_name).copied() {
                Some(kind) => kind,
                None => {
                    return Err(syn::Error::new_spanned(
                        &section_name,
                        error_msg::UNMAPPED_SECTION,
                    ));
                }
            };

            // Start building the Variants, nameless variants are numerated
            let mut variants = Vec::<Variant>::new();
            let mut nameless_ident = 0u32;

            // Parse the content to build Variants, we either have named or nameless variants
            while !content.is_empty() {
                // Parse of kind syn::Ident => { }
                let (name, ty) = if content.peek(syn::Ident) && content.peek2(Token![=>]) {
                    let name: syn::Ident = content.parse()?;
                    content.parse::<Token![=>]>()?;

                    let ty = parse_ty(kind, &content)?;

                    (name, ty)

                // Parse nameless variants, for now atomic naming, switch to path named
                } else {
                    let name = format_ident!("v{}", nameless_ident);
                    nameless_ident += 1;

                    let ty = parse_ty(kind, &content)?;

                    (name, ty)
                };

                // Ignore commas
                if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                }

                if variants.iter().any(|variant| variant.name == name) {
                    return Err(syn::Error::new_spanned(&name, error_msg::DUPLICATE_VARIANT));
                }

                variants.push(Variant { name, ty });
            }

            sections.push(Section {
                name: section_name,
                variants,
            });
        }

        while !input.is_empty() {
            input.step(|cursor| {
                cursor
                    .token_tree()
                    .map(|(_, next)| ((), next))
                    .ok_or(cursor.error(error_msg::EXPECTED_FUNCTION_DECLARATION))
            })?;
        }

        Ok(MozartInput { function, sections })
    }
}

// Gets an untouched mozart! input, and finds the function definition
fn get_function_from_stream(input: ParseStream) -> syn::Result<syn::ItemFn> {
    let fork = input.fork();

    while !looks_like_fn_item(&fork) {
        fork.step(|cursor| {
            cursor
                .token_tree()
                .map(|(_, next)| ((), next))
                .ok_or(cursor.error(error_msg::EXPECTED_FUNCTION_DECLARATION))
        })?;
    }

    let function: syn::ItemFn = fork.parse()?;

    if !fork.is_empty() {
        return Err(fork.error(error_msg::UNEXPECTED_TOKENS_AFTER_FUNCTION));
    }

    Ok(function)
}

fn looks_like_fn_item(input: ParseStream) -> bool {
    let fork = input.fork();
    let _ = fork.call(syn::Attribute::parse_outer);
    let _ = fork.parse::<syn::Visibility>();
    fork.parse::<syn::Signature>().is_ok()
}

// Tries to parse the content into a VariantType, and returns the span
fn parse_ty(kind: VariantKind, input: ParseStream) -> syn::Result<VariantType> {
    let ty = match kind {
        VariantKind::Expr => VariantType::Expr(input.parse()?),
        VariantKind::Type => VariantType::Type(input.parse()?),
        VariantKind::Block => VariantType::Block(input.parse()?),
    };

    Ok(ty)
}

#[derive(Default)]
struct VariantMapping {
    mapping: HashMap<syn::Ident, VariantKind>,
    error: Option<syn::Error>,
}

impl VariantMapping {
    fn build(i: &syn::ItemFn) -> syn::Result<HashMap<syn::Ident, VariantKind>> {
        let mut mapper = Self::default();
        mapper.visit_item_fn(i);

        match mapper.error {
            Some(err) => Err(err),
            None => Ok(mapper.mapping),
        }
    }

    fn push_error<T, U>(&mut self, tokens: T, message: U)
    where
        T: ToTokens,
        U: Display,
    {
        let err = syn::Error::new_spanned(&tokens, message);
        match &mut self.error {
            Some(existing) => existing.combine(err),
            None => self.error = Some(err),
        }
    }
}

impl<'ast> syn::visit::Visit<'ast> for VariantMapping {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("variant") {
            let kind = match mac.delimiter {
                syn::MacroDelimiter::Brace(_) => VariantKind::Block,
                syn::MacroDelimiter::Bracket(_) => VariantKind::Expr,
                syn::MacroDelimiter::Paren(_) => VariantKind::Type,
            };

            let name: syn::Ident = match syn::parse2(mac.tokens.clone()) {
                Ok(ident) => ident,
                Err(_) => {
                    self.push_error(&mac.tokens, error_msg::VARIANT_NAME_TOKEN_MISMATCH);
                    return;
                }
            };

            match self.mapping.entry(name.clone()) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    v.insert(kind);
                }

                std::collections::hash_map::Entry::Occupied(o) => {
                    if *o.get() != kind {
                        self.push_error(&name, error_msg::VARIANT_USED_WITH_MULTIPLE_KINDS);
                    }
                }
            }
        }
    }
}

type Combination<'a> = HashMap<&'a syn::Ident, &'a Variant>;

fn compose(input: MozartInput) -> syn::Result<proc_macro2::TokenStream> {
    let combinations = cartesian_product(&input.sections);

    let fn_sig = input.function.sig.clone();

    if !fn_sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &fn_sig.generics,
            error_msg::UNSUPPORTED_GENERIC_FUNCTION,
        ));
    }

    if let Some(asyncness) = fn_sig.asyncness {
        return Err(syn::Error::new_spanned(
            &asyncness,
            error_msg::UNSUPPORTED_ASYNC_FUNCTION,
        ));
    }

    let generated_fns = combinations
        .iter()
        .map(|combination| compose_function(&input.function, combination))
        .collect::<syn::Result<Vec<_>>>()?;

    let accesor = compose_accessor(&fn_sig, &generated_fns)?;

    let mod_ident = format_ident!("__variants_{}", &fn_sig.ident);

    Ok(quote! {
        mod #mod_ident {
            #![allow(dead_code, non_snake_case, unused_variables, unused_mut)]
            use super::*;

            #(#generated_fns)*

            #accesor
        }
    })
}

fn cartesian_product(sections: &[Section]) -> Vec<Combination<'_>> {
    fn rec<'a>(
        idx: usize,
        sections: &'a [Section],
        current: &mut Combination<'a>,
        out: &mut Vec<Combination<'a>>,
    ) {
        if idx == sections.len() {
            out.push(current.clone());
            return;
        }

        let section = &sections[idx];

        for variant in &section.variants {
            current.insert(&section.name, variant);

            rec(idx + 1, sections, current, out);

            current.remove(&section.name);
        }
    }

    if sections.is_empty() {
        return vec![HashMap::new()];
    }

    let mut out = Vec::new();
    let mut current = HashMap::new();

    rec(0, sections, &mut current, &mut out);

    out
}

fn compose_function(function: &syn::ItemFn, combination: &Combination) -> syn::Result<syn::ItemFn> {
    let mut function = function.clone();

    let mut entries: Vec<_> = combination.iter().collect();
    entries.sort_by_key(|(name, _)| name.to_string());

    let suffix = entries
        .iter()
        .map(|(_, variant)| format!("{}", variant.name))
        .collect::<Vec<_>>()
        .join("_");

    function.sig.ident = format_ident!("__{}_{}", function.sig.ident, suffix);

    let mut replacer = EntryReplacer {
        combination,
        error: None,
    };
    replacer.visit_item_fn_mut(&mut function);

    match replacer.error {
        Some(err) => Err(err),
        None => Ok(function),
    }
}

fn compose_accessor(
    sig: &syn::Signature,
    functions: &[syn::ItemFn],
) -> syn::Result<proc_macro2::TokenStream> {
    let idents: Vec<syn::Ident> = functions
        .iter()
        .map(|func| func.sig.ident.clone())
        .collect();

    let fn_type = {
        let arg_types = sig
            .inputs
            .iter()
            .map(|arg| match arg {
                syn::FnArg::Typed(pat_ty) => Ok(&pat_ty.ty),
                syn::FnArg::Receiver(_) => {
                    Err(syn::Error::new_spanned(&arg, error_msg::UNSUPPORTED_METHOD))
                }
            })
            .collect::<syn::Result<Vec<_>>>()?;

        let output = sig.output.clone();

        quote! {
            fn(#(#arg_types),*) #output
        }
    };

    let unsafety = sig.unsafety;

    Ok(quote! {
        type Rhayader = #unsafety #fn_type;
        pub fn accessor()
            -> impl Iterator<Item = self::Rhayader>
        {
            [#(self::#idents as self::Rhayader),*].into_iter()
        }
    })
}

struct EntryReplacer<'a> {
    combination: &'a Combination<'a>,
    error: Option<syn::Error>,
}

impl<'a> EntryReplacer<'a> {
    fn push_error<T, U>(&mut self, tokens: T, message: U)
    where
        T: ToTokens,
        U: Display,
    {
        let err = syn::Error::new_spanned(&tokens, message);
        match &mut self.error {
            Some(existing) => existing.combine(err),
            None => self.error = Some(err),
        }
    }

    fn push_pure_error(&mut self, err: syn::Error) {
        match &mut self.error {
            Some(existing) => existing.combine(err),
            None => self.error = Some(err),
        }
    }
}

impl<'a> VisitMut for EntryReplacer<'a> {
    fn visit_expr_mut(&mut self, node: &mut syn::Expr) {
        if let syn::Expr::Macro(expr_mac) = node {
            if expr_mac.mac.path.is_ident("variant") {
                let name: syn::Ident = match syn::parse2(expr_mac.mac.tokens.clone()) {
                    Ok(ident) => ident,
                    Err(err) => {
                        self.push_pure_error(err);
                        return;
                    }
                };

                let variant = match self.combination.get(&name) {
                    Some(v) => v,
                    None => {
                        self.push_error(name, error_msg::UNKNOWN_EXPR_VARIANT_GROUP);
                        return;
                    }
                };

                match expr_mac.mac.delimiter {
                    syn::MacroDelimiter::Bracket(_) => {
                        let expr: syn::Expr = (&**variant).into();
                        *node = expr;
                        return;
                    }
                    syn::MacroDelimiter::Paren(_) => {
                        self.push_error(&expr_mac.mac, error_msg::TYPE_VARIANT_IN_EXPR_POSITION);
                    }
                    syn::MacroDelimiter::Brace(_) => match &variant.ty {
                        VariantType::Block(b) => {
                            let expr = syn::Expr::Block(syn::ExprBlock {
                                attrs: Vec::new(),
                                label: None,
                                block: b.clone(),
                            });
                            *node = expr;
                            return;
                        }
                        _ => {
                            self.push_error(expr_mac, error_msg::BLOCK_PLACEHOLDER_KIND_MISMATCH);
                            return;
                        }
                    },
                }
            }
        }

        syn::visit_mut::visit_expr_mut(self, node);
    }

    fn visit_type_mut(&mut self, node: &mut syn::Type) {
        if let syn::Type::Macro(type_mac) = node {
            if type_mac.mac.path.is_ident("variant") {
                let name: syn::Ident = match syn::parse2(type_mac.mac.tokens.clone()) {
                    Ok(ident) => ident,
                    Err(err) => {
                        self.push_pure_error(err);
                        return;
                    }
                };

                let variant = match self.combination.get(&name) {
                    Some(v) => v,
                    None => {
                        self.push_error(name, error_msg::UNKNOWN_TYPE_VARIANT_GROUP);
                        return;
                    }
                };

                match type_mac.mac.delimiter {
                    syn::MacroDelimiter::Paren(_) => {
                        let ty: syn::Type = (&**variant).into();
                        *node = ty;
                        return;
                    }
                    _ => {
                        self.push_error(type_mac, error_msg::NON_TYPE_VARIANT_IN_TYPE_POSITION);
                        return;
                    }
                }
            }
        }

        syn::visit_mut::visit_type_mut(self, node);
    }

    fn visit_stmt_mut(&mut self, node: &mut syn::Stmt) {
        if let syn::Stmt::Macro(stmt_macro) = node {
            if stmt_macro.mac.path.is_ident("variant") {
                let name: syn::Ident = match syn::parse2(stmt_macro.mac.tokens.clone()) {
                    Ok(ident) => ident,
                    Err(err) => {
                        self.push_pure_error(err);
                        return;
                    }
                };

                let variant = match self.combination.get(&name) {
                    Some(v) => v,
                    None => {
                        self.push_error(name, error_msg::UNKNOWN_STMT_VARIANT_GROUP);
                        return;
                    }
                };

                match stmt_macro.mac.delimiter {
                    syn::MacroDelimiter::Brace(_) => {
                        *node = syn::Stmt::Expr((&**variant).into(), None);
                        return;
                    }
                    syn::MacroDelimiter::Bracket(_) => {
                        let expr: syn::Expr = (&**variant).into();
                        *node = parse_quote! { #expr ; };
                        return;
                    }
                    syn::MacroDelimiter::Paren(_) => {
                        self.push_error(&stmt_macro.mac, error_msg::TYPE_VARIANT_IN_STMT_POSITION);
                    }
                }
            }
        }

        syn::visit_mut::visit_stmt_mut(self, node);
    }
}

mod error_msg {
    pub const DUPLICATE_SECTION: &str = "Variant section was declared before";
    pub const EMPTY_SECTION: &str = "Variant section is empty";
    pub const UNMAPPED_SECTION: &str =
        "Variant section was not referenced by any variant! placeholder in the function";
    pub const DUPLICATE_VARIANT: &str = "Variant was declared before";
    pub const EXPECTED_FUNCTION_DECLARATION: &str =
        "expected a function declaration, perhaps you forgot ?";
    pub const UNEXPECTED_TOKENS_AFTER_FUNCTION: &str =
        "Unexpected trailing tokens after function declaration. Try a diary instead";
    pub const VARIANT_NAME_TOKEN_MISMATCH: &str =
        "variant! placeholder must contain exactly one identifier";
    pub const VARIANT_USED_WITH_MULTIPLE_KINDS: &str =
        "Variant group used with multiple placeholder kinds";
    pub const UNSUPPORTED_GENERIC_FUNCTION: &str =
        "mozart! does not support generic functions (including lifetime parameters)";
    pub const UNSUPPORTED_ASYNC_FUNCTION: &str = "mozart! does not support `async fn`";
    pub const UNSUPPORTED_METHOD: &str = "methods are not supported";
    pub const UNKNOWN_EXPR_VARIANT_GROUP: &str = "Unknown variant group in expression position";
    pub const TYPE_VARIANT_IN_EXPR_POSITION: &str = "variant!(name) with parentheses denotes a type group and cannot be used in expression position; use variant![name] instead";
    pub const BLOCK_PLACEHOLDER_KIND_MISMATCH: &str =
        "variant! block placeholder resolved to a non-block variant";
    pub const UNKNOWN_TYPE_VARIANT_GROUP: &str = "Unknown variant group in type position";
    pub const NON_TYPE_VARIANT_IN_TYPE_POSITION: &str =
        "Only variant!(name) type placeholders can be used in type position";
    pub const UNKNOWN_STMT_VARIANT_GROUP: &str = "Unknown variant group in statement position";
    pub const TYPE_VARIANT_IN_STMT_POSITION: &str =
        "variant!(name) type placeholder cannot be used in statement position";
}
