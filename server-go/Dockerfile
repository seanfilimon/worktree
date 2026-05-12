FROM golang:1.25-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o /wt-server ./cmd/wt-server

FROM alpine:3.20
RUN apk add --no-cache ca-certificates
COPY --from=build /wt-server /usr/local/bin/wt-server
EXPOSE 8080 9877
ENTRYPOINT ["wt-server"]
