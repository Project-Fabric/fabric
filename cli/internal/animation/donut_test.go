package animation

import (
	"strings"
	"testing"
)

func TestFrameDimensionsAndMotion(t *testing.T) {
	a := Frame(48, 18, 0)
	b := Frame(48, 18, .9)
	if a == b {
		t.Fatal("frames must rotate")
	}
	lines := strings.Split(a, "\n")
	if len(lines) != 18 {
		t.Fatal(len(lines))
	}
	visible := 0
	for _, line := range lines {
		if len(line) != 48 {
			t.Fatal("wrong width")
		}
		visible += len(strings.TrimSpace(line))
	}
	if visible < 30 {
		t.Fatal("torus is empty")
	}
}
func TestTinyAndBoundedFrames(t *testing.T) {
	for _, size := range [][2]int{{0, 0}, {1, 1}, {2, 40}, {100000, 100000}, {-1, 10}} {
		text := Frame(size[0], size[1], 1.2)
		if len(text) > 4040 {
			t.Fatal("unbounded allocation")
		}
	}
}
