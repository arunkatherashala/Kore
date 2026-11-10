"""Release compatibility gate (pytest wrapper around compat_check.py, which also runs without pytest:
python -I kore-python/compat_check.py fixtures --local)."""
import compat_check
import kore_fileformat as kore


def test_old_fixtures_open_with_this_module_and_match_the_pinned_data():
    problems = compat_check.check_fixtures(kore)
    assert not problems, '\n'.join(problems)


def test_files_written_now_read_back_and_future_versions_are_refused(tmp_path):
    compat_check.write_cases(kore, tmp_path)
    problems = compat_check.read_cases(tmp_path, local=True)
    assert not problems, '\n'.join(problems)
