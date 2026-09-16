//! `#[derive(Report)]` — named field 구조체에 `::netsci_report::Report` 구현을 생성한다.
//!
//! proc-macro 크레이트는 트레이트를 export 할 수 없으므로 트레이트는 `netsci-report` 에 있고,
//! 생성 코드는 `::netsci_report::Report` 절대 경로를 참조한다.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Field, Fields, LitInt, LitStr, Type, parse_macro_input};

const NAMED_FIELDS_ONLY: &str = "Report can only be derived for structs with named fields";

/// `Report` 구현을 생성한다.
///
/// 필드 속성:
/// - `#[report(rename = "name")]` — 열 이름 변경
/// - `#[report(skip)]` — 열에서 제외 (다른 속성과 함께 쓸 수 없다)
/// - `#[report(precision = N)]` — `format!("{:.N}")` 적용
///
/// `Option<T>` 필드는 `None` 이면 빈 문자열이 된다.
#[proc_macro_derive(Report, attributes(report))]
pub fn derive_report(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            Fields::Unnamed(unnamed) => {
                return Err(syn::Error::new_spanned(unnamed, NAMED_FIELDS_ONLY));
            }
            Fields::Unit => return Err(syn::Error::new_spanned(input, NAMED_FIELDS_ONLY)),
        },
        Data::Enum(data) => return Err(syn::Error::new(data.enum_token.span, NAMED_FIELDS_ONLY)),
        Data::Union(data) => {
            return Err(syn::Error::new(data.union_token.span, NAMED_FIELDS_ONLY));
        }
    };

    // 필드마다 속성을 파싱하고, 에러는 모아서 한 번에 보고한다.
    let mut columns = Vec::new();
    let mut errors: Option<syn::Error> = None;
    for field in fields {
        match FieldOptions::parse(field) {
            Ok(options) if options.skip => {}
            Ok(options) => columns.push(Column::new(field, options)),
            Err(err) => match &mut errors {
                Some(all) => all.combine(err),
                None => errors = Some(err),
            },
        }
    }
    if let Some(err) = errors {
        return Err(err);
    }

    let headers = columns.iter().map(|c| &c.header);
    let cells = columns.iter().map(Column::cell);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::netsci_report::Report for #name #ty_generics #where_clause {
            fn headers() -> ::std::vec::Vec<&'static str> {
                ::std::vec![#(#headers),*]
            }

            fn row(&self) -> ::std::vec::Vec<::std::string::String> {
                ::std::vec![#(#cells),*]
            }
        }
    })
}

/// `#[report(...)]` 속성 파싱 결과.
#[derive(Default)]
struct FieldOptions {
    rename: Option<LitStr>,
    precision: Option<LitInt>,
    skip: bool,
}

impl FieldOptions {
    fn parse(field: &Field) -> syn::Result<Self> {
        let mut options = Self::default();
        // skip 과 다른 속성의 혼용을 잡으려고 처음 본 속성 경로를 기억한다.
        let mut skip_path = None;
        let mut other_path = None;

        for attr in field.attrs.iter().filter(|a| a.path().is_ident("report")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("skip") {
                    if options.skip {
                        return Err(meta.error("duplicate `skip` attribute"));
                    }
                    if other_path.is_some() {
                        return Err(meta.error("`skip` cannot be combined with other report attributes"));
                    }
                    options.skip = true;
                    skip_path = Some(meta.path.clone());
                    return Ok(());
                }

                if meta.path.is_ident("rename") {
                    if options.rename.is_some() {
                        return Err(meta.error("duplicate `rename` attribute"));
                    }
                    options.rename = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("precision") {
                    if options.precision.is_some() {
                        return Err(meta.error("duplicate `precision` attribute"));
                    }
                    let lit: LitInt = meta.value()?.parse()?;
                    lit.base10_parse::<usize>()?;
                    options.precision = Some(lit);
                } else {
                    let key = meta.path.to_token_stream().to_string().replace(' ', "");
                    return Err(meta.error(format!(
                        "unknown report attribute `{key}`; expected `rename`, `skip`, or `precision`"
                    )));
                }

                if skip_path.is_some() {
                    return Err(meta.error("`skip` cannot be combined with other report attributes"));
                }
                other_path = Some(meta.path.clone());
                Ok(())
            })?;
        }
        Ok(options)
    }
}

/// 출력에 들어가는 열 하나.
struct Column {
    header: LitStr,
    ident: syn::Ident,
    precision: Option<usize>,
    is_option: bool,
}

impl Column {
    fn new(field: &Field, options: FieldOptions) -> Self {
        // named field 만 여기로 오므로 ident 는 항상 있다. 방어적으로 빈 이름을 쓴다.
        let ident = field
            .ident
            .clone()
            .unwrap_or_else(|| syn::Ident::new("_", field.span()));
        let header = options
            .rename
            .unwrap_or_else(|| LitStr::new(&ident.unraw().to_string(), ident.span()));
        let precision = options
            .precision
            .and_then(|lit| lit.base10_parse::<usize>().ok());
        Self {
            header,
            ident,
            precision,
            is_option: is_option(&field.ty),
        }
    }

    /// 셀 문자열을 만드는 식.
    fn cell(&self) -> TokenStream2 {
        let ident = &self.ident;
        let format_value = |value: TokenStream2| match self.precision {
            Some(p) => {
                let fmt = format!("{{:.{p}}}");
                quote!(::std::format!(#fmt, #value))
            }
            None => quote!(::std::string::ToString::to_string(#value)),
        };
        if self.is_option {
            let some = format_value(quote!(value));
            quote! {
                match &self.#ident {
                    ::std::option::Option::Some(value) => #some,
                    ::std::option::Option::None => ::std::string::String::new(),
                }
            }
        } else {
            format_value(quote!(&self.#ident))
        }
    }
}

/// 타입 경로의 끝 세그먼트가 `Option` 인지 본다 (`Option<T>`, `std::option::Option<T>` 등).
fn is_option(ty: &Type) -> bool {
    match ty {
        Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Option"),
        _ => false,
    }
}
