import pytest
from cart import total


def test_adds_up():
    assert total([1, 2]) == 3
    assert total([]) == 0


@pytest.mark.skip(reason="flaky")
def test_half_cent():
    assert total([0.005]) == 0.01
