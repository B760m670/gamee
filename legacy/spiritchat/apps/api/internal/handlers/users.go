package handlers

import (
	"encoding/json"
	"log"
	"net/http"

	"github.com/mysocialapp/api/internal/middleware"
	"github.com/mysocialapp/api/internal/services"
)

type UserHandler struct {
	sb     *services.SupabaseClient
	tokens *services.TokenIssuer
}

func NewUserHandler(sb *services.SupabaseClient, tokens *services.TokenIssuer) *UserHandler {
	return &UserHandler{sb: sb, tokens: tokens}
}

func (h *UserHandler) GetMe(w http.ResponseWriter, r *http.Request) {
	userID, _ := middleware.UserIDFromContext(r.Context())

	var users []map[string]interface{}
	if err := h.sb.Select(r.Context(), "users", "id=eq."+userID, &users); err != nil {
		log.Printf("get me: %v", err)
		jsonErr(w, "failed to fetch user", http.StatusInternalServerError)
		return
	}
	if len(users) == 0 {
		jsonErr(w, "user not found", http.StatusNotFound)
		return
	}
	jsonOK(w, map[string]interface{}{"data": users[0]})
}

func (h *UserHandler) UpdateMe(w http.ResponseWriter, r *http.Request) {
	userID, _ := middleware.UserIDFromContext(r.Context())

	var body map[string]interface{}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}

	allowed := map[string]bool{
		"display_name": true, "bio": true, "avatar_url": true,
		"website": true, "username": true, "onboarded": true,
	}
	update := make(map[string]interface{})
	for k, v := range body {
		if allowed[k] {
			update[k] = v
		}
	}
	if len(update) == 0 {
		jsonErr(w, "no updatable fields provided", http.StatusBadRequest)
		return
	}

	var updated []map[string]interface{}
	if err := h.sb.Update(r.Context(), "users", "id=eq."+userID, update, &updated); err != nil {
		log.Printf("update me: %v", err)
		jsonErr(w, "failed to update user", http.StatusInternalServerError)
		return
	}
	if len(updated) == 0 {
		jsonErr(w, "user not found", http.StatusNotFound)
		return
	}
	jsonOK(w, map[string]interface{}{"data": updated[0]})
}
