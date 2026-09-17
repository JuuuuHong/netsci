use netsci_report::Report;

struct Doi(String);

#[derive(Report)]
struct Row {
    rank: usize,
    doi: Doi,
    maybe: Option<Doi>,
}

fn main() {}
