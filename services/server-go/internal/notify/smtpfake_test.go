package notify

import (
	"bufio"
	"net"
	"strings"
	"sync"
	"testing"
)

// fakeSMTP is a minimal in-process SMTP server (EHLO, optional AUTH
// PLAIN, MAIL, RCPT, DATA, QUIT). failFirst makes the first failFirst DATA
// commands answer 451 to exercise retries.
type fakeSMTP struct {
	ln net.Listener

	mu        sync.Mutex
	failFirst int
	auth      bool
	messages  []fakeMessage
	got       chan fakeMessage
}

type fakeMessage struct {
	From string
	To   []string
	Data string
	Auth string // the AUTH PLAIN argument, if any
}

func newFakeSMTP(t *testing.T, failFirst int, auth bool) *fakeSMTP {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	s := &fakeSMTP{ln: ln, failFirst: failFirst, auth: auth, got: make(chan fakeMessage, 100)}
	go func() {
		for {
			c, err := ln.Accept()
			if err != nil {
				return
			}
			go s.serve(c)
		}
	}()
	t.Cleanup(func() { ln.Close() })
	return s
}

func (s *fakeSMTP) addr() string { return s.ln.Addr().String() }

func (s *fakeSMTP) list() []fakeMessage {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]fakeMessage(nil), s.messages...)
}

func (s *fakeSMTP) serve(c net.Conn) {
	defer c.Close()
	r := bufio.NewReader(c)
	w := func(line string) { _, _ = c.Write([]byte(line + "\r\n")) }
	w("220 fake.test ESMTP")
	var msg fakeMessage
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			return
		}
		line = strings.TrimRight(line, "\r\n")
		cmd := strings.ToUpper(line)
		switch {
		case strings.HasPrefix(cmd, "EHLO"), strings.HasPrefix(cmd, "HELO"):
			if s.auth {
				w("250-fake.test")
				w("250 AUTH PLAIN")
			} else {
				w("250 fake.test")
			}
		case strings.HasPrefix(cmd, "AUTH PLAIN"):
			msg.Auth = strings.TrimSpace(line[len("AUTH PLAIN"):])
			w("235 ok")
		case strings.HasPrefix(cmd, "MAIL FROM:"):
			msg.From = strings.Trim(line[len("MAIL FROM:"):], "<> ")
			w("250 ok")
		case strings.HasPrefix(cmd, "RCPT TO:"):
			msg.To = append(msg.To, strings.Trim(line[len("RCPT TO:"):], "<> "))
			w("250 ok")
		case cmd == "DATA":
			s.mu.Lock()
			fail := s.failFirst > 0
			if fail {
				s.failFirst--
			}
			s.mu.Unlock()
			if fail {
				w("451 try again later")
				continue
			}
			w("354 go ahead")
			var b strings.Builder
			for {
				l, err := r.ReadString('\n')
				if err != nil {
					return
				}
				if l == ".\r\n" || l == ".\n" {
					break
				}
				b.WriteString(strings.TrimPrefix(l, "."))
			}
			msg.Data = b.String()
			s.mu.Lock()
			s.messages = append(s.messages, msg)
			s.mu.Unlock()
			s.got <- msg
			msg = fakeMessage{}
			w("250 queued")
		case cmd == "RSET":
			msg = fakeMessage{}
			w("250 ok")
		case cmd == "QUIT":
			w("221 bye")
			return
		default:
			w("502 unknown command")
		}
	}
}
