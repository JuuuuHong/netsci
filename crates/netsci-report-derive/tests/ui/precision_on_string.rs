use netsci_report::Report;

#[derive(Report)]
struct Row {
    #[report(precision = 3)]
    name: String,
    #[report(precision = 2)]
    count: Option<u32>,
}

fn main() {}
