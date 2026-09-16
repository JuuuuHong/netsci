use netsci_report::Report;

#[derive(Report)]
struct Row {
    rank: usize,
    #[report(skip, rename = "id")]
    internal_id: u32,
}

fn main() {}
