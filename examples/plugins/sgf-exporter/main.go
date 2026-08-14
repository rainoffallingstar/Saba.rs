package main

import (
	"bufio"
	"encoding/binary"
	"encoding/json"
	"io"
	"os"
)

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      uint64          `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
}

type response struct {
	JSONRPC string      `json:"jsonrpc"`
	ID      uint64      `json:"id"`
	Result  interface{} `json:"result,omitempty"`
}

func readFrame(reader io.Reader) ([]byte, error) {
	var payloadLength uint32
	if err := binary.Read(reader, binary.LittleEndian, &payloadLength); err != nil {
		return nil, err
	}

	payload := make([]byte, payloadLength)
	if _, err := io.ReadFull(reader, payload); err != nil {
		return nil, err
	}
	return payload, nil
}

func writeFrame(writer io.Writer, payload interface{}) error {
	serializedPayload, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	if err := binary.Write(writer, binary.LittleEndian, uint32(len(serializedPayload))); err != nil {
		return err
	}
	_, err = writer.Write(serializedPayload)
	return err
}

func main() {
	input := bufio.NewReader(os.Stdin)
	output := bufio.NewWriter(os.Stdout)
	defer output.Flush()

	for {
		payload, err := readFrame(input)
		if err != nil {
			return
		}

		var pluginRequest request
		if err := json.Unmarshal(payload, &pluginRequest); err != nil {
			return
		}

		if err := writeFrame(output, response{
			JSONRPC: "2.0",
			ID:      pluginRequest.ID,
			Result: map[string]string{
				"status": "ready",
			},
		}); err != nil {
			return
		}
		output.Flush()
	}
}
