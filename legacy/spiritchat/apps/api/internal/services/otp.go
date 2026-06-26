package services

import (
	"context"
	"crypto/rand"
	"errors"
	"math/big"
	"net/url"
	"time"
)

var (
	ErrResendTooSoon   = errors.New("please wait before requesting a new code")
	ErrCodeExpired     = errors.New("code expired or not found")
	ErrCodeIncorrect   = errors.New("incorrect code")
	ErrTooManyAttempts = errors.New("too many attempts, request a new code")
)

type OTPService struct {
	db            *SupabaseClient
	ttlSeconds    int
	cooldown      int
	maxAttempts   int
	codeLength    int
}

func NewOTPService(db *SupabaseClient, ttl, cooldown, maxAttempts, codeLength int) *OTPService {
	return &OTPService{db: db, ttlSeconds: ttl, cooldown: cooldown, maxAttempts: maxAttempts, codeLength: codeLength}
}

type otpRow struct {
	Email       string    `json:"email"`
	Code        string    `json:"code"`
	ExpiresAt   time.Time `json:"expires_at"`
	ResendAfter time.Time `json:"resend_after"`
	Attempts    int       `json:"attempts"`
}

func (s *OTPService) Generate(ctx context.Context, email string) (string, error) {
	now := time.Now().UTC()

	var existing []otpRow
	if err := s.db.Select(ctx, "otp_codes", "email=eq."+url.QueryEscape(email)+"&select=resend_after", &existing); err != nil {
		return "", err
	}
	if len(existing) > 0 && existing[0].ResendAfter.After(now) {
		return "", ErrResendTooSoon
	}

	code, err := randomCode(s.codeLength)
	if err != nil {
		return "", err
	}

	row := otpRow{
		Email:       email,
		Code:        code,
		ExpiresAt:   now.Add(time.Duration(s.ttlSeconds) * time.Second),
		ResendAfter: now.Add(time.Duration(s.cooldown) * time.Second),
		Attempts:    0,
	}
	if err := s.db.Upsert(ctx, "otp_codes", row); err != nil {
		return "", err
	}
	return code, nil
}

func (s *OTPService) Verify(ctx context.Context, email, code string) error {
	var rows []otpRow
	if err := s.db.Select(ctx, "otp_codes", "email=eq."+url.QueryEscape(email), &rows); err != nil {
		return err
	}
	if len(rows) == 0 || rows[0].ExpiresAt.Before(time.Now().UTC()) {
		return ErrCodeExpired
	}

	row := rows[0]
	if row.Code != code {
		attempts := row.Attempts + 1
		if attempts >= s.maxAttempts {
			_ = s.db.Delete(ctx, "otp_codes", "email=eq."+url.QueryEscape(email))
			return ErrTooManyAttempts
		}
		_ = s.db.Update(ctx, "otp_codes", "email=eq."+url.QueryEscape(email), map[string]int{"attempts": attempts}, nil)
		return ErrCodeIncorrect
	}

	_ = s.db.Delete(ctx, "otp_codes", "email=eq."+url.QueryEscape(email))
	return nil
}

func randomCode(length int) (string, error) {
	const digits = "0123456789"
	code := make([]byte, length)
	for i := range code {
		n, err := rand.Int(rand.Reader, big.NewInt(int64(len(digits))))
		if err != nil {
			return "", err
		}
		code[i] = digits[n.Int64()]
	}
	return string(code), nil
}
