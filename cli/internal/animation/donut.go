// Package animation renders an ASCII torus using perspective projection and a
// reciprocal-depth buffer. Mathematical reference: Andy Sloane, 2011,
// https://www.a1k0n.net/2011/07/20/donut-math.html . This is an original Go renderer.
package animation

import (
	"math"
	"strings"
)

func Frame(width, height int, phase float64) string {
	if width < 1 || height < 1 {
		return ""
	}
	width = min(width, 100)
	height = min(height, 40)
	pixels := make([]byte, width*height)
	for i := range pixels {
		pixels[i] = ' '
	}
	depth := make([]float64, len(pixels))
	a, b := phase*.7, phase*.35
	ca, sa, cb, sb := math.Cos(a), math.Sin(a), math.Cos(b), math.Sin(b)
	rotate := func(x, y, z float64) (float64, float64, float64) {
		y, z = y*ca-z*sa, y*sa+z*ca
		return x*cb - y*sb, x*sb + y*cb, z
	}
	scale := math.Min(float64(width)*.63, float64(height)*1.25)
	const shades = ".,-~:;=!*#$@"
	for theta := 0.; theta < 2*math.Pi; theta += .09 {
		ct, st := math.Cos(theta), math.Sin(theta)
		for phi := 0.; phi < 2*math.Pi; phi += .035 {
			cp, sp := math.Cos(phi), math.Sin(phi)
			x, y, z := rotate((2+ct)*cp, (2+ct)*sp, st)
			// Characters are approximately twice as tall as they are wide.
			inv := 1 / (z + 7)
			px := int(float64(width)/2 + scale*x*inv*2)
			py := int(float64(height)/2 - scale*y*inv)
			if px < 0 || px >= width || py < 0 || py >= height {
				continue
			}
			index := py*width + px
			if inv <= depth[index] {
				continue
			}
			nx, ny, nz := rotate(ct*cp, ct*sp, st)
			light := (nx*.2 + ny*.7 - nz*.7) / 1.01
			shade := int((light + 1) * .5 * float64(len(shades)-1))
			shade = max(0, min(len(shades)-1, shade))
			pixels[index] = shades[shade]
			depth[index] = inv
		}
	}
	lines := make([]string, height)
	for row := range lines {
		lines[row] = string(pixels[row*width : (row+1)*width])
	}
	return strings.Join(lines, "\n")
}
