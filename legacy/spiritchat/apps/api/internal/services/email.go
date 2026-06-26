package services

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"time"
)

type EmailSender interface {
	Send(ctx context.Context, to, subject, body string) error
}

type consoleEmailSender struct{}

func (consoleEmailSender) Send(_ context.Context, to, _, body string) error {
	log.Printf("[EMAIL] to=%s | %s", to, body)
	return nil
}

type resendSender struct {
	apiKey string
	from   string
	http   *http.Client
}

func (r *resendSender) Send(ctx context.Context, to, subject, body string) error {
	payload, _ := json.Marshal(map[string]interface{}{
		"from":    r.from,
		"to":      []string{to},
		"subject": subject,
		"text":    body,
	})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "https://api.resend.com/emails", bytes.NewReader(payload))
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+r.apiKey)
	req.Header.Set("Content-Type", "application/json")

	resp, err := r.http.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	if resp.StatusCode >= 400 {
		b, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("resend %d: %s", resp.StatusCode, string(b))
	}
	return nil
}

func NewEmailSender(provider, apiKey, from string) EmailSender {
	if provider == "resend" {
		return &resendSender{apiKey: apiKey, from: from, http: &http.Client{Timeout: 10 * time.Second}}
	}
	return consoleEmailSender{}
}
