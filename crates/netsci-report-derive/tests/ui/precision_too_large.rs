use netsci_report::Report;

#[derive(Report)]
struct Row {
    #[report(precision = 100000)]
    score: f64,
}

fn main() {}
