use netsci_report::Report;

#[derive(Report)]
struct Row {
    #[report(display, precision = 2)]
    score: f64,
}

fn main() {}
