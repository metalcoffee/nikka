#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

#![deny(warnings)]

extern crate proc_macro;

use std::ops::Deref;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{
    ToTokens,
    quote,
    quote_spanned,
};
use syn::{
    Abi,
    Error,
    FnArg,
    Ident,
    ItemFn,
    LitStr,
    Pat,
    PatIdent,
    PatType,
    ReturnType,
    Type,
    TypePath,
    parse_macro_input,
    spanned::Spanned,
    token::Extern,
};

enum MethodReturnType {
    Unit,
    Primitive(syn::Path),
    Never,
}

/// Обертка точки входа для корректных трассировок стека.
///
/// Перед вызовом данной функции добавляет стековый фрейм (`rbp = 0`, `return_address = 0`),
/// который означает окончание стека.
///
/// # Examples
///
/// ```
/// use sentinel_frame::with_sentinel_frame;
/// # struct BootInfo;
///
/// #[with_sentinel_frame]
/// fn kernel_entry_point(boot_info: &'static BootInfo) -> ! {
/// #   panic!()
///     // ...
/// }
/// ```
#[proc_macro_attribute]
pub fn with_sentinel_frame(
    _attr: TokenStream,
    item: TokenStream,
) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    let vis = input_fn.vis;
    let attrs = input_fn.attrs;
    let sig = input_fn.sig;
    let original_fn_name = &sig.ident;
    let fn_block = input_fn.block;

    let mut errors = Vec::new();

    let inner_f_name = Ident::new(
        &format!("{original_fn_name}_inner"),
        original_fn_name.span(),
    );

    let mut inner_sig = sig.clone();
    inner_sig.ident = inner_f_name.clone();

    if let Some(abi) = inner_sig.abi.take() &&
        abi.name.as_ref().map(LitStr::value).as_deref() != Some("C")
    {
        errors.push(Error::new_spanned(
            abi,
            "Only default and C calling conventions are supported",
        ));
    }
    inner_sig.abi = Some(Abi {
        extern_token: Extern {
            span: original_fn_name.span(),
        },
        name: Some(LitStr::new("C", original_fn_name.span())),
    });

    let mut asm_args = Vec::new();
    const ARG_REGISTERS: [&str; 6] = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
    let inputs = &sig.inputs;

    let return_type = match check_return_type(&sig.output) {
        Ok(ret) => ret,
        Err(e) => return e.to_compile_error().into(),
    };

    if inputs.len() > ARG_REGISTERS.len() {
        errors.push(Error::new_spanned(
            inputs,
            format!(
                "Functions with more than {} arguments are not supported",
                ARG_REGISTERS.len()
            ),
        ));
    }

    for (arg, reg) in inputs.iter().zip(ARG_REGISTERS) {
        match arg {
            FnArg::Typed(PatType { pat, ty, .. }) => {
                if let Err(e) = check_argument_type(ty) {
                    errors.push(e);
                }

                let arg_name = match pat.deref() {
                    Pat::Ident(PatIdent { ident, .. }) => ident,
                    _ => {
                        errors.push(Error::new_spanned(
                            pat,
                            "Complex patterns are not supported in function arguments",
                        ));
                        continue;
                    },
                };

                asm_args.push(quote_spanned! { arg.span() => in(#reg) #arg_name });
            },
            FnArg::Receiver(recv) => {
                errors.push(Error::new_spanned(recv, "Receivers are not supported"));
            },
        }
    }

    if !errors.is_empty() {
        let combined_errors = errors
            .into_iter()
            .map(|e| e.to_compile_error())
            .collect::<proc_macro2::TokenStream>();
        return combined_errors.into();
    }

    let output = Ident::new("output", Span::call_site());
    match return_type {
        MethodReturnType::Primitive(_) => asm_args.push(quote! { lateout("rax") #output }),
        MethodReturnType::Never => asm_args.push(quote! { options(noreturn) }),
        MethodReturnType::Unit => {},
    };

    let asm_block = quote! {
        unsafe {
            core::arch::asm!("
                push rbp
                sub rsp, 8
                push 0
                push 0
                mov rbp, rsp
                call {func}
                add rsp, 24
                pop rbp
                ",
                func = sym #inner_f_name,
                #(#asm_args,)*
                clobber_abi("C")
            );
        }
    };

    let fn_invocation = match return_type {
        MethodReturnType::Unit | MethodReturnType::Never => asm_block,
        MethodReturnType::Primitive(path) => quote! {
            let mut #output: #path;
            #asm_block
            #output
        },
    };

    quote! {
        #(#attrs)*
        #vis #sig {
            // Inner function
            #inner_sig {
                #fn_block
            }

            #fn_invocation
        }
    }
    .into()
}

fn input_type_error<T: ToTokens>(tokens: T) -> Error {
    Error::new_spanned(
        tokens,
        "Only primitive integer types and references are allowed",
    )
}

fn check_argument_type(ty: &Type) -> Result<(), Error> {
    match ty {
        Type::Path(TypePath { path, .. }) => check_primitive_type(path),
        Type::Reference(ref_type) => check_reference_type(&ref_type.elem),
        Type::Ptr(ptr_type) => check_reference_type(&ptr_type.elem),
        _ => Err(input_type_error(ty)),
    }
}

fn check_return_type(ty: &ReturnType) -> Result<MethodReturnType, Error> {
    let ty = match ty {
        ReturnType::Default => return Ok(MethodReturnType::Unit),
        ReturnType::Type(_, ty) => ty,
    };

    match ty.deref() {
        Type::Path(TypePath { path, .. }) => {
            check_primitive_type(path)?;
            Ok(MethodReturnType::Primitive(path.clone()))
        },
        Type::Never(_) => Ok(MethodReturnType::Never),
        Type::Tuple(t) if t.elems.is_empty() => Ok(MethodReturnType::Unit),
        _ => Err(Error::new_spanned(
            ty,
            "Only primitive types, unit type and never (!) are allowed as return types",
        )),
    }
}

fn check_primitive_type(path: &syn::Path) -> Result<(), Error> {
    const ALLOWED_TYPES: [&str; 10] = [
        "i8",
        "u8",
        "i16",
        "u16",
        "i32",
        "u32",
        "i64",
        "u64",
        "isize",
        "usize",
    ];
    let indent = path
        .get_ident()
        .ok_or_else(|| Error::new_spanned(path, "Argument type should be specified explicitly"))?;
    if !ALLOWED_TYPES.contains(&indent.to_string().as_str()) {
        return Err(input_type_error(path));
    }
    Ok(())
}

fn check_reference_type(ty: &Type) -> Result<(), Error> {
    // TODO: maybe prohibit impl Trait as there can be `[T]` or `dyn Trait` hiding behind it
    match ty {
        Type::TraitObject(_) => Err(Error::new_spanned(ty, "dyn Trait types are not supported")),
        Type::Paren(_) => Err(Error::new_spanned(ty, "Remove unnecessaty parenthesis")),
        Type::Slice(_) => Err(Error::new_spanned(ty, "Slices are not supported")),
        _ => Ok(()),
    }
}
