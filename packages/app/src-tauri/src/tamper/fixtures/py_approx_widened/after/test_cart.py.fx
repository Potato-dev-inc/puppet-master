import pytest
from cart import total


def test_half_cent():
    assert total([0.005]) == pytest.approx(0.01, rel=1e-1)
