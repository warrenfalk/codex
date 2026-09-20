import unittest
from dataclasses import replace
from unittest.mock import patch

import format as formatter


class PythonFormatterWrapperTest(unittest.TestCase):
    def test_wrapper_preserves_uv_project_and_formatter_arguments(self) -> None:
        wrapper = "/formatter tools/ruff-wrapper"
        for check in (False, True):
            for build_group in (
                formatter.python_sdk_formatter_group,
                formatter.python_scripts_formatter_group,
            ):
                with self.subTest(check=check, group=build_group.__name__):
                    with patch.object(formatter.os, "environ", {}):
                        original = build_group(check=check)
                    with patch.object(
                        formatter.os, "environ", {"CODEX_RUFF_WRAPPER": wrapper}
                    ):
                        wrapped = build_group(check=check)

                    expected = replace(
                        original,
                        commands=tuple(
                            replace(
                                command,
                                args=tuple(
                                    wrapper if arg == "ruff" else arg
                                    for arg in command.args
                                ),
                            )
                            for command in original.commands
                        ),
                    )
                    self.assertEqual(wrapped, expected)


if __name__ == "__main__":
    unittest.main()
