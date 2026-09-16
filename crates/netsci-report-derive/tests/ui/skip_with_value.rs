use netsci_report::Report;

#[derive(Report)]
struct Row {
    #[report(skip = true)]
    internal_id: u32,
}

fn main() {}
