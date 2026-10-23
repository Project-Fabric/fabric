package engine

import (
	"encoding/json"
	"math/big"
	"regexp"
	"strings"
)

// The engine declarations identify units; amounts are never inferred from tool names.
// The legacy inbox fixture is the one explicitly documented exception (USD cents).
func ApprovalAmount(a Approval, declarations []Declaration, fixture bool) (string, bool) {
	if fixture && a.Target == "stripe/issue_refund" {
		return amount(Field(a.Input, "amount"), "USD", true)
	}
	var match *Declaration
	// Fabric uses the first matching declaration, including broad declarations.
	for i := range declarations {
		d := &declarations[i]
		if d.Tool != a.Target {
			continue
		}
		ok := true
		for _, w := range d.When {
			value, isString := Field(a.Input, w.Field).(string)
			if !isString || value != w.Equals {
				ok = false
			}
		}
		if ok {
			match = d
			break
		}
	}
	if match != nil {
		currency := Value(match.DefaultCurrency)
		if match.Currency != nil {
			if v, ok := Field(a.Input, *match.Currency).(string); ok && v != "" {
				currency = v
			}
		}
		return amount(Field(a.Input, match.Amount), currency, match.MinorUnits)
	}
	return "Amount / units unavailable", false
}
func amount(value any, currency string, minor bool) (string, bool) {
	currency = strings.ToUpper(currency)
	// Deliberately bounded supported exponents. Unknown currencies fail closed.
	exponent, ok := map[string]int{"USD": 2, "EUR": 2, "GBP": 2, "INR": 2, "CAD": 2, "AUD": 2, "CHF": 2, "SGD": 2, "JPY": 0, "KRW": 0, "BHD": 3, "KWD": 3}[currency]
	if !ok {
		return "Unknown currency / units", false
	}
	var text string
	switch v := value.(type) {
	case json.Number:
		text = v.String()
	case string:
		text = v
	default:
		return "Amount unavailable", false
	}
	if !decimal.MatchString(text) {
		return "Invalid or unsupported amount", false
	}
	n, ok := new(big.Rat).SetString(text)
	if !ok || n.Sign() < 0 {
		return "Invalid amount", false
	}
	if minor {
		if !n.IsInt() {
			return "Invalid minor-unit amount", false
		}
		scale := new(big.Int).Exp(big.NewInt(10), big.NewInt(int64(exponent)), nil)
		n.Quo(n, new(big.Rat).SetInt(scale))
	}
	// Reject values that would need rounding at the declared currency precision.
	rendered := n.FloatString(exponent)
	parsed, _ := new(big.Rat).SetString(rendered)
	if parsed.Cmp(n) != 0 {
		return "Amount exceeds currency precision", false
	}
	return currency + " " + rendered, true
}

var decimal = regexp.MustCompile(`^[0-9]{1,30}(\.[0-9]{1,6})?$`)
