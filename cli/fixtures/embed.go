// Package fixtures embeds captured, sanitized engine responses for a portable demo binary.
package fixtures

import "embed"

//go:embed *.json
var Files embed.FS
