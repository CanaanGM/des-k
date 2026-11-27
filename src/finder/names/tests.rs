// The name tests: a description that is a file name, and a Store app's name.
use super::*;

// An app's name is shown to you in notes and the finder, so a file name as description must
// read as the app, while a name that only ends in letters like "exe" stays whole.
#[test]
fn a_description_that_is_a_file_name_loses_its_exe() {
    assert_eq!(no_exe("Notepad.exe"), "Notepad");
    assert_eq!(no_exe("PAINT.EXE"), "PAINT");
    assert_eq!(no_exe("Microsoft Edge"), "Microsoft Edge");
    assert_eq!(no_exe("Flexe"), "Flexe");
    assert_eq!(no_exe(".exe"), "");
    assert_eq!(no_exe("Ünï"), "Ünï");
}

// Spec 0027 AC-13: a hidden Store app is suspended, so its name must come from the rule
// key alone, which des-k holds lowercased. Needs Calculator installed: run with --ignored.
#[test]
#[ignore]
fn store_name_reads_a_lowercased_app_id() {
    assert_eq!(store_name("microsoft.windowscalculator_8wekyb3d8bbwe!app").as_deref(), Some("Windows Calculator"));
    assert_eq!(store_name("no.such.package_0000000000000!app"), None);
    assert_eq!(store_name("notepad.exe"), None);
}
