package services

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

type SupabaseClient struct {
	baseURL    string
	serviceKey string
	http       *http.Client
}

func NewSupabaseClient(baseURL, serviceKey string) *SupabaseClient {
	return &SupabaseClient{
		baseURL:    baseURL,
		serviceKey: serviceKey,
		http:       &http.Client{Timeout: 10 * time.Second},
	}
}

func (c *SupabaseClient) do(ctx context.Context, method, table, filter string, body interface{}, headers map[string]string) ([]byte, error) {
	url := c.baseURL + "/rest/v1/" + table
	if filter != "" {
		url += "?" + filter
	}

	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		bodyReader = bytes.NewReader(data)
	}

	req, err := http.NewRequestWithContext(ctx, method, url, bodyReader)
	if err != nil {
		return nil, err
	}
	req.Header.Set("apikey", c.serviceKey)
	req.Header.Set("Authorization", "Bearer "+c.serviceKey)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	for k, v := range headers {
		req.Header.Set(k, v)
	}

	resp, err := c.http.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	b, _ := io.ReadAll(resp.Body)
	if resp.StatusCode >= 400 {
		return nil, fmt.Errorf("supabase %d: %s", resp.StatusCode, string(b))
	}
	return b, nil
}

func (c *SupabaseClient) Select(ctx context.Context, table, filter string, out interface{}) error {
	b, err := c.do(ctx, http.MethodGet, table, filter, nil, nil)
	if err != nil {
		return err
	}
	return json.Unmarshal(b, out)
}

func (c *SupabaseClient) Insert(ctx context.Context, table string, body interface{}, out interface{}) error {
	b, err := c.do(ctx, http.MethodPost, table, "", body, map[string]string{"Prefer": "return=representation"})
	if err != nil {
		return err
	}
	if out != nil {
		return json.Unmarshal(b, out)
	}
	return nil
}

func (c *SupabaseClient) Upsert(ctx context.Context, table string, body interface{}) error {
	_, err := c.do(ctx, http.MethodPost, table, "", body, map[string]string{
		"Prefer": "resolution=merge-duplicates",
	})
	return err
}

func (c *SupabaseClient) Update(ctx context.Context, table, filter string, body interface{}, out interface{}) error {
	b, err := c.do(ctx, http.MethodPatch, table, filter, body, map[string]string{"Prefer": "return=representation"})
	if err != nil {
		return err
	}
	if out != nil {
		return json.Unmarshal(b, out)
	}
	return nil
}

func (c *SupabaseClient) Delete(ctx context.Context, table, filter string) error {
	_, err := c.do(ctx, http.MethodDelete, table, filter, nil, nil)
	return err
}
