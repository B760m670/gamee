package main

import (
	"log"
	"net/http"

	"github.com/mysocialapp/api/internal/config"
	"github.com/mysocialapp/api/internal/handlers"
	"github.com/mysocialapp/api/internal/middleware"
	"github.com/mysocialapp/api/internal/services"
)

func main() {
	cfg := config.Load()

	sb := services.NewSupabaseClient(cfg.SupabaseURL, cfg.SupabaseServiceKey)
	mailer := services.NewEmailSender(cfg.EmailProvider, cfg.ResendAPIKey, cfg.ResendFrom)
	tokens := services.NewTokenIssuer(cfg.JWTSecret, cfg.AccessTokenTTL)
	otp := services.NewOTPService(sb, cfg.OTPTTLSeconds, cfg.OTPResendCooldown, cfg.OTPMaxAttempts, cfg.OTPCodeLength)

	auth := handlers.NewAuthHandler(sb, otp, mailer, tokens, cfg.OTPDevExposeCode)
	users := handlers.NewUserHandler(sb, tokens)
	requireAuth := middleware.RequireAuth(tokens)

	mux := http.NewServeMux()

	mux.HandleFunc("POST /api/v1/auth/send-otp", auth.SendOTP)
	mux.HandleFunc("POST /api/v1/auth/verify-otp", auth.VerifyOTP)
	mux.Handle("GET /api/v1/users/me", requireAuth(http.HandlerFunc(users.GetMe)))
	mux.Handle("PUT /api/v1/users/me", requireAuth(http.HandlerFunc(users.UpdateMe)))

	log.Printf("starting server on :%s (env=%s)", cfg.Port, cfg.Environment)
	log.Fatal(http.ListenAndServe(":"+cfg.Port, corsMiddleware(mux)))
}

func corsMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Access-Control-Allow-Origin", "*")
		w.Header().Set("Access-Control-Allow-Headers", "authorization, content-type")
		w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusOK)
			return
		}
		next.ServeHTTP(w, r)
	})
}
