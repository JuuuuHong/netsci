use netsci_report::Report;

#[derive(Report)]
struct Row {
    rank: usize,
    #[report(foo)]
    name: String,
}

fn main() {}
