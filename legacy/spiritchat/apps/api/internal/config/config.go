package config

import (
	"os"
	"strconv"
	"time"
)

type Config struct {
	Port               string
	Environment        string
	SupabaseURL        string
	SupabaseServiceKey string
	JWTSecret          string
	AccessTokenTTL     time.Duration

	OTPTTLSeconds     int
	OTPResendCooldown int
	OTPMaxAttempts    int
	OTPCodeLength     int
	OTPDevExposeCode  bool

	EmailProvider string
	ResendAPIKey  string
	ResendFrom    string
}

func Load() *Config {
	env := getEnv("ENVIRONMENT", "development")
	return &Config{
		Port:               getEnv("PORT", "8080"),
		Environment:        env,
		SupabaseURL:        os.Getenv("SUPABASE_URL"),
		SupabaseServiceKey: os.Getenv("SUPABASE_SERVICE_KEY"),
		JWTSecret:          os.Getenv("JWT_SECRET"),
		AccessTokenTTL:     time.Duration(getEnvInt("ACCESS_TOKEN_TTL_HOURS", 24*30)) * time.Hour,

		OTPTTLSeconds:     getEnvInt("OTP_TTL_SECONDS", 300),
		OTPResendCooldown: getEnvInt("OTP_RESEND_COOLDOWN_SECONDS", 60),
		OTPMaxAttempts:    getEnvInt("OTP_MAX_ATTEMPTS", 5),
		OTPCodeLength:     getEnvInt("OTP_CODE_LENGTH", 6),
		OTPDevExposeCode:  getEnvBool("OTP_DEV_EXPOSE_CODE", env != "production"),

		EmailProvider: getEnv("EMAIL_PROVIDER", "console"),
		ResendAPIKey:  os.Getenv("RESEND_API_KEY"),
		ResendFrom:    getEnv("RESEND_FROM", "MySocialApp <noreply@mysocialapp.com>"),
	}
}

func getEnv(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func getEnvInt(key string, fallback int) int {
	if v := os.Getenv(key); v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			return n
		}
	}
	return fallback
}

func getEnvBool(key string, fallback bool) bool {
	if v := os.Getenv(key); v != "" {
		if b, err := strconv.ParseBool(v); err == nil {
			return b
		}
	}
	return fallback
}
