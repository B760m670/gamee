package handlers

import (
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"net/url"
	"regexp"
	"strings"

	"github.com/google/uuid"
	"github.com/mysocialapp/api/internal/services"
)

var emailRe = regexp.MustCompile(`^[^\s@]+@[^\s@]+\.[^\s@]+$`)

type AuthHandler struct {
	sb         *services.SupabaseClient
	otp        *services.OTPService
	mailer     services.EmailSender
	tokens     *services.TokenIssuer
	exposeCode bool
}

func NewAuthHandler(sb *services.SupabaseClient, otp *services.OTPService, mailer services.EmailSender, tokens *services.TokenIssuer, exposeCode bool) *AuthHandler {
	return &AuthHandler{sb: sb, otp: otp, mailer: mailer, tokens: tokens, exposeCode: exposeCode}
}

func (h *AuthHandler) SendOTP(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Email string `json:"email"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	email := strings.ToLower(strings.TrimSpace(req.Email))
	if !emailRe.MatchString(email) {
		jsonErr(w, "invalid email address", http.StatusBadRequest)
		return
	}

	code, err := h.otp.Generate(r.Context(), email)
	if err != nil {
		if errors.Is(err, services.ErrResendTooSoon) {
			jsonErr(w, err.Error(), http.StatusTooManyRequests)
			return
		}
		log.Printf("otp generate: %v", err)
		jsonErr(w, "failed to generate code", http.StatusInternalServerError)
		return
	}

	subject := "Your verification code"
	body := "Your MySocialApp verification code is: " + code + "\n\nExpires in 5 minutes. Do not share it with anyone."
	if err := h.mailer.Send(r.Context(), email, subject, body); err != nil {
		log.Printf("send email: %v", err)
		if !h.exposeCode {
			jsonErr(w, "failed to send email", http.StatusBadGateway)
			return
		}
	}

	resp := map[string]interface{}{"sent": true}
	if h.exposeCode {
		resp["dev_code"] = code
	}
	jsonOK(w, resp)
}

func (h *AuthHandler) VerifyOTP(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Email string `json:"email"`
		Code  string `json:"code"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	email := strings.ToLower(strings.TrimSpace(req.Email))
	if !emailRe.MatchString(email) {
		jsonErr(w, "invalid email address", http.StatusBadRequest)
		return
	}
	if req.Code == "" {
		jsonErr(w, "code is required", http.StatusBadRequest)
		return
	}

	if err := h.otp.Verify(r.Context(), email, req.Code); err != nil {
		switch {
		case errors.Is(err, services.ErrCodeExpired):
			jsonErr(w, err.Error(), http.StatusUnauthorized)
		case errors.Is(err, services.ErrCodeIncorrect):
			jsonErr(w, err.Error(), http.StatusUnauthorized)
		case errors.Is(err, services.ErrTooManyAttempts):
			jsonErr(w, err.Error(), http.StatusTooManyRequests)
		default:
			log.Printf("otp verify: %v", err)
			jsonErr(w, "verification failed", http.StatusInternalServerError)
		}
		return
	}

	var users []map[string]interface{}
	if err := h.sb.Select(r.Context(), "users", "email=eq."+url.QueryEscape(email), &users); err != nil {
		log.Printf("find user: %v", err)
		jsonErr(w, "lookup failed", http.StatusInternalServerError)
		return
	}

	isNew := false
	var user map[string]interface{}

	if len(users) == 0 {
		var created []map[string]interface{}
		err := h.sb.Insert(r.Context(), "users", map[string]interface{}{
			"id":        uuid.New().String(),
			"email":     email,
			"onboarded": false,
		}, &created)
		if err != nil {
			log.Printf("create user: %v", err)
			jsonErr(w, "failed to create user", http.StatusInternalServerError)
			return
		}
		user = created[0]
		isNew = true
	} else {
		user = users[0]
	}

	token, err := h.tokens.Issue(user["id"].(string))
	if err != nil {
		log.Printf("issue token: %v", err)
		jsonErr(w, "failed to issue token", http.StatusInternalServerError)
		return
	}

	jsonOK(w, map[string]interface{}{"token": token, "user": user, "is_new": isNew})
}
