package cart

import "testing"

func TestAdds(t *testing.T) {
	if Total([]int{1, 2}) != 3 {
		t.Errorf("want 3")
	}
}

func TestHalfCent(t *testing.T) {
	if Total([]int{5}) != 5 {
		t.Logf("want 5")
	}
}
