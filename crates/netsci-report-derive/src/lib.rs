//! `#[derive(Report)]` — named field 구조체에 `::netsci_report::Report` 구현을 생성한다.
//!
//! proc-macro 크레이트는 트레이트를 export 할 수 없으므로 트레이트는 `netsci-report` 에 있고,
//! 생성 코드는 `::netsci_report::Report` 절대 경로를 참조한다.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote, quote_spanned};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Field, Fields, LitInt, LitStr, parse_macro_input};

const NAMED_FIELDS_ONLY: &str = "Report can only be derived for structs with named fields";

/// `Report` 구현을 생성한다.
///
/// 필드 속성:
/// - `#[report(rename = "name")]` — 열 이름 변경
/// - `#[report(skip)]` — 열에서 제외 (다른 속성과 함께 쓸 수 없다)
/// - `#[report(precision = N)]` — 소수 N 자리. 필드 타입이 `netsci_report::PrecisionCell`(`f32`·`f64`·그 `Option`·참조)을 구현해야 한다
/// - `#[report(display)]` — `Display` 로 바로 문자열화한다. `Cell` 을 구현하지 않은 사용자 정의 타입용이다 (`precision` 과 함께 쓸 수 없다)
///
/// 매크로는 필드 타입을 토큰으로 판별하지 않는다. 속성이 없는 필드는 `netsci_report::Cell::cell`,
/// `precision` 필드는 `netsci_report::PrecisionCell::cell_with_precision` 호출을 필드 타입의 span 으로 만들고,
/// 타입이 맞는지는 트레이트 구현으로 컴파일러가 판정한다. 그래서 타입 별칭(`type S = Option<f64>`)도 실제 타입대로
/// 처리되고, 맞지 않는 타입의 에러는 필드 타입 위치에 난다. `Option<T>` 의 `None` 은 트레이트 구현에서 빈 문자열이 된다.
///
/// 제네릭 구조체에는 바운드를 더하지 않는다. 타입 매개변수 필드를 쓰려면 `T: netsci_report::Cell` 을 직접 적는다.
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
    /// `display` 경로 (에러 위치용)
    display: Option<syn::Path>,
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
                    if meta.input.peek(syn::Token![=]) || meta.input.peek(syn::token::Paren) {
                        return Err(meta.error("`skip` takes no value; write `#[report(skip)]`"));
                    }
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
                    // format! 정밀도는 u16 범위여야 한다. 넘으면 rustc 가 derive 위치에 엉뚱한 에러를 낸다.
                    if lit.base10_parse::<u16>().is_err() {
                        return Err(syn::Error::new_spanned(
                            &lit,
                            "`precision` must be an integer between 0 and 65535",
                        ));
                    }
                    options.precision = Some(lit);
                } else if meta.path.is_ident("display") {
                    if meta.input.peek(syn::Token![=]) || meta.input.peek(syn::token::Paren) {
                        return Err(meta.error("`display` takes no value; write `#[report(display)]`"));
                    }
                    if options.display.is_some() {
                        return Err(meta.error("duplicate `display` attribute"));
                    }
                    options.display = Some(meta.path.clone());
                } else {
                    let key = meta.path.to_token_stream().to_string().replace(' ', "");
                    return Err(meta.error(format!(
                        "unknown report attribute `{key}`; expected `rename`, `skip`, `precision`, or `display`"
                    )));
                }

                if skip_path.is_some() {
                    return Err(meta.error("`skip` cannot be combined with other report attributes"));
                }
                other_path = Some(meta.path.clone());
                Ok(())
            })?;
        }
        // 자릿수는 `Display` 로 바로 문자열화하는 경로에 적용할 수 없다 (문자열이면 잘린다).
        if let (Some(_), Some(path)) = (&options.precision, &options.display) {
            return Err(syn::Error::new_spanned(
                path,
                "`display` cannot be combined with `precision`",
            ));
        }
        Ok(options)
    }
}

/// 셀 문자열을 만드는 방식.
enum CellKind {
    /// `<T as ::netsci_report::Cell>::cell`
    Cell,
    /// `<T as ::netsci_report::PrecisionCell>::cell_with_precision`
    Precision(u16),
    /// `<T as ::std::string::ToString>::to_string`
    Display,
}

/// 출력에 들어가는 열 하나.
struct Column {
    header: LitStr,
    ident: syn::Ident,
    ty: syn::Type,
    kind: CellKind,
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
            .and_then(|lit| lit.base10_parse::<u16>().ok());
        let kind = match (precision, options.display) {
            (Some(p), _) => CellKind::Precision(p),
            (None, Some(_)) => CellKind::Display,
            (None, None) => CellKind::Cell,
        };
        Self {
            header,
            ident,
            ty: strip_parens(&field.ty).clone(),
            kind,
        }
    }

    /// 셀 문자열을 만드는 식. 필드 타입을 `<T as Trait>` 로 명시해 호출한다.
    ///
    /// - 타입이 트레이트를 구현하지 않으면 rustc 가 한정 경로 안의 `T` 토큰, 즉 사용자가 쓴 필드 타입 위치에 에러를 낸다.
    ///   `Cell::cell(&self.field)` 처럼 `Self` 를 추론에 맡기면 주 에러 위치가 인자, 곧 `#[derive(Report)]` 가 된다
    /// - 한정 경로의 나머지 토큰도 `quote_spanned!` 로 필드 타입 span 을 주고, 매크로가 만든 `&self.field` 는 호출 위치 span 으로 둔다
    fn cell(&self) -> TokenStream2 {
        let ident = &self.ident;
        let ty = &self.ty;
        let value = quote!(&self.#ident);
        let span = ty.span();
        match self.kind {
            CellKind::Cell => {
                quote_spanned!(span=> <#ty as ::netsci_report::Cell>::cell(#value))
            }
            CellKind::Precision(p) => {
                let precision = usize::from(p);
                quote_spanned!(span=> <#ty as ::netsci_report::PrecisionCell>::cell_with_precision(#value, #precision))
            }
            CellKind::Display => {
                quote_spanned!(span=> <#ty as ::std::string::ToString>::to_string(#value))
            }
        }
    }
}

/// `(T)` 의 괄호를 벗긴다. 괄호째 `<(T) as Trait>` 로 내보내면 사용자 코드 span 에 `unused_parens` 경고가 난다.
fn strip_parens(ty: &syn::Type) -> &syn::Type {
    match ty {
        syn::Type::Paren(paren) => strip_parens(&paren.elem),
        other => other,
    }
}
