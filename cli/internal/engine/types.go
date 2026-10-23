package engine

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
	"strings"
	"unicode"
)

type Object map[string]any

type Approval struct {
	ID              uint64  `json:"id"`
	Digest          string  `json:"digest"`
	Rule            string  `json:"rule"`
	Reason          string  `json:"reason"`
	Agent           string  `json:"agent"`
	Session         *string `json:"session"`
	Tool            string  `json:"tool"`
	Target          string  `json:"target"`
	Input           Object  `json:"input"`
	RequestedAt     string  `json:"requested_at"`
	RequestedOSUser *string `json:"requested_os_user"`
	Status          string  `json:"status"`
	Approver        *string `json:"approver"`
	DecidedAt       *string `json:"decided_at"`
	Note            *string `json:"note"`
}
type Approvals struct {
	Schema   string     `json:"schema"`
	Requests []Approval `json:"requests"`
}
type Payment struct {
	Amount            string  `json:"amount"`
	Currency          string  `json:"currency"`
	Decision          string  `json:"decision"`
	AuthorizationCode *string `json:"authorization_code"`
	Consent           *string `json:"consent"`
	Reason            *string `json:"reason"`
}
type Call struct {
	Record    uint64   `json:"record"`
	At        string   `json:"at"`
	Agent     string   `json:"agent"`
	Tool      string   `json:"tool"`
	Arguments *string  `json:"arguments"`
	Decision  string   `json:"decision"`
	Outcome   string   `json:"outcome"`
	Result    *string  `json:"result"`
	Rule      *string  `json:"rule"`
	Session   *string  `json:"session"`
	Payment   *Payment `json:"payment"`
}
type Calls struct {
	Schema string `json:"schema"`
	Calls  []Call `json:"calls"`
}
type Agent struct {
	Name      string   `json:"name"`
	Tools     []string `json:"tools"`
	CreatedAt string   `json:"created_at"`
	RevokedAt *string  `json:"revoked_at"`
}
type Agents struct {
	Schema   string  `json:"schema"`
	Required bool    `json:"required"`
	Agents   []Agent `json:"agents"`
}
type Terms struct {
	Title          string   `json:"title"`
	Tools          []string `json:"tools"`
	Agent          *string  `json:"agent"`
	Expires        *string  `json:"expires"`
	MaxPerCall     *string  `json:"max_per_call"`
	MaxPerDay      *string  `json:"max_per_day"`
	MaxCallsPerDay *uint64  `json:"max_calls_per_day"`
}
type Use struct {
	Day    string `json:"day"`
	Amount *int64 `json:"amount"`
}
type Mandate struct {
	ID        uint64  `json:"id"`
	Terms     Terms   `json:"terms"`
	GrantedAt string  `json:"granted_at"`
	GrantedBy string  `json:"granted_by"`
	RevokedAt *string `json:"revoked_at"`
	Uses      []Use   `json:"uses"`
}
type Mandates struct {
	Schema   string    `json:"schema"`
	Mandates []Mandate `json:"mandates"`
}
type Checks struct {
	Checked  uint64   `json:"checked"`
	Problems []string `json:"problems"`
}
type Verification struct {
	Schema string `json:"schema"`
	Deep   bool   `json:"deep"`
	Chain  struct {
		OK         bool     `json:"ok"`
		Records    uint64   `json:"records"`
		Head       *string  `json:"head"`
		Incomplete bool     `json:"incomplete_tail"`
		Problems   []string `json:"problems"`
	} `json:"chain"`
	Exceptions  []any  `json:"exceptions"`
	Signed      Checks `json:"signed_decisions"`
	MandateUses Checks `json:"mandate_uses"`
}

func (v Verification) OK() bool {
	return v.Chain.OK && !v.Chain.Incomplete && len(v.Chain.Problems)+len(v.Exceptions)+len(v.Signed.Problems)+len(v.MandateUses.Problems) == 0
}

type Declaration struct {
	Tool            string  `json:"tool"`
	Amount          string  `json:"amount"`
	Currency        *string `json:"currency"`
	DefaultCurrency *string `json:"default_currency"`
	MinorUnits      bool    `json:"minor_units"`
	Merchant        *string `json:"merchant"`
	When            []struct {
		Field  string `json:"field"`
		Equals string `json:"equals"`
	} `json:"when"`
}
type Declarations struct {
	Schema   string        `json:"schema"`
	Payments []Declaration `json:"payments"`
}
type Consent struct {
	ID          string `json:"consent_id"`
	Principal   string `json:"user_id"`
	Description string `json:"intent_description"`
	Constraints struct {
		Max      json.Number `json:"max_amount"`
		Currency string      `json:"currency"`
	} `json:"constraints"`
	Scope struct {
		Single bool `json:"single_use"`
	} `json:"scope"`
	Active  bool    `json:"is_active"`
	Expires *string `json:"expires_at"`
}
type Consents struct {
	Consents []Consent `json:"consents"`
	Total    uint64    `json:"total"`
}
type Snapshot struct {
	History      History
	Rules        Rules
	Changes      Changes
	Approvals    Approvals
	Calls        Calls
	Agents       Agents
	Mandates     Mandates
	Verification Verification
	Declarations Declarations
	Consents     Consents
	Errors       map[string]string
}
type Action struct {
	Kind           string
	ID             uint64
	Digest         string
	Reject         bool
	Note           string
	Name           string
	Tools          []string
	Title          string
	Agent          string
	MaxPerCall     string
	MaxPerDay      string
	MaxCallsPerDay uint64
	Expires        string
	Out            string
}
type Receipt struct {
	Detail      string
	Simulated   bool
	Args        []string
	Message     string
	Token       string
	Out         string
	Fingerprint string
}

func Value(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}
func Field(input Object, path string) any {
	var value any = input
	for _, key := range strings.Split(path, ".") {
		switch object := value.(type) {
		case Object:
			value = object[key]
		case map[string]any:
			value = object[key]
		default:
			return nil
		}
	}
	return value
}
func Input(call Call) Object {
	var input Object
	if call.Arguments != nil {
		_ = Decode([]byte(*call.Arguments), &input)
	}
	return input
}
func Decode(data []byte, output any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := decoder.Decode(output); err != nil {
		return fmt.Errorf("invalid engine JSON: %w", err)
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return errors.New("engine returned trailing or multiple JSON values")
	}
	return nil
}
func CheckSchema(data []byte, schema string, output any) error {
	var header struct {
		Schema string `json:"schema"`
		Error  struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := Decode(data, &header); err != nil {
		return err
	}
	if header.Schema == "fabric.error/v1" {
		return errors.New(Clean(header.Error.Message))
	}
	if header.Schema != schema {
		return fmt.Errorf("unsupported engine schema %q; expected %s", Clean(header.Schema), schema)
	}
	return Decode(data, output)
}

var credentials = regexp.MustCompile(`(?:sk|rk)_(?:test|live)_[A-Za-z0-9_]+|fat_[A-Za-z0-9_-]+|aa_live_[A-Za-z0-9_-]+|eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`)
var ansiSequences = regexp.MustCompile("\\x1b(?:\\[[0-?]*[ -/]*[@-~]|\\][^\\x07\\x1b]*(?:\\x07|\\x1b\\\\))")
var sensitive = regexp.MustCompile(`(?i)secret|token|password|api.?key|authorization|signature|private.?key`)

// Clean prevents both credential leakage and terminal escape/OSC injection from tool output.
func Clean(text string) string {
	text = ansiSequences.ReplaceAllString(text, "")
	text = credentials.ReplaceAllString(text, "[redacted]")
	return strings.Map(func(r rune) rune {
		if unicode.IsControl(r) && r != '\n' && r != '\t' {
			return -1
		}
		return r
	}, text)
}
func Redact(value any) any {
	switch item := value.(type) {
	case string:
		var nested any
		if json.Valid([]byte(item)) && Decode([]byte(item), &nested) == nil {
			switch nested.(type) {
			case map[string]any, []any:
				raw, _ := json.Marshal(Redact(nested))
				return Clean(string(raw))
			}
		}
		return Clean(item)
	case map[string]any:
		result := map[string]any{}
		for key, value := range item {
			if key != "authorization_code" && sensitive.MatchString(key) {
				result[key] = "[redacted]"
			} else {
				result[key] = Redact(value)
			}
		}
		return result
	case Object:
		return Redact(map[string]any(item))
	case []any:
		result := make([]any, len(item))
		for i, value := range item {
			result[i] = Redact(value)
		}
		return result
	default:
		return value
	}
}
func SafeJSON(value any) string {
	raw, err := json.Marshal(value)
	if err != nil {
		return "Unable to render input"
	}
	var decoded any
	_ = Decode(raw, &decoded)
	out, _ := json.MarshalIndent(Redact(decoded), "", "  ")
	return string(out)
}
