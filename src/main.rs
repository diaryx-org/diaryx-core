use diaryx_core::cli::DiaryxCli;

fn main() {
    let diaryxcli = DiaryxCli::from_args();
    diaryxcli.print_config();
}
