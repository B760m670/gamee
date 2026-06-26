package middleware

import (
	"context"
	"net/http"
	"strings"

	"github.com/mysocialapp/api/internal/services"
)

type contextKey string

const userIDKey contextKey = "userID"

func RequireAuth(tokens *services.TokenIssuer) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			parts := strings.SplitN(r.Header.Get("Authorization"), " ", 2)
			if len(parts) != 2 || !strings.EqualFold(parts[0], "bearer") {
				jsonUnauthorized(w)
				return
			}
			userID, err := tokens.Verify(parts[1])
			if err != nil {
				jsonUnauthorized(w)
				return
			}
			next.ServeHTTP(w, r.WithContext(context.WithValue(r.Context(), userIDKey, userID)))
		})
	}
}

func UserIDFromContext(ctx context.Context) (string, bool) {
	id, ok := ctx.Value(userIDKey).(string)
	return id, ok
}

func jsonUnauthorized(w http.ResponseWriter) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusUnauthorized)
	w.Write([]byte(`{"message":"unauthorized"}`)) //nolint:errcheck
}
