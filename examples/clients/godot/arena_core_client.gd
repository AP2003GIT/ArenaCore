extends Node

signal player_created(player: Dictionary)
signal matchmaking_updated(status: Dictionary)
signal request_failed(status_code: int, response_body: String)

@export var base_url := "http://127.0.0.1:3000"


func create_player(display_name: String, rating: int = 1200) -> void:
	var body := JSON.stringify({"display_name": display_name, "rating": rating})
	var response := await _json_request("/api/v1/players", HTTPClient.METHOD_POST, body)
	_emit_result(response, player_created)


func enqueue(player_id: String, mode: String = "1v1") -> void:
	var body := JSON.stringify({"player_id": player_id, "mode": mode})
	var response := await _json_request("/api/v1/matchmaking/enqueue", HTTPClient.METHOD_POST, body)
	_emit_result(response, matchmaking_updated)


func _json_request(path: String, method: HTTPClient.Method, body: String) -> Dictionary:
	var request := HTTPRequest.new()
	add_child(request)
	request.timeout = 10.0
	var error := request.request(
		base_url.trim_suffix("/") + path,
		["Content-Type: application/json"],
		method,
		body
	)
	if error != OK:
		request.queue_free()
		return {"status": 0, "body": "request could not be started"}

	var completed: Array = await request.request_completed
	request.queue_free()
	return {
		"status": int(completed[1]),
		"body": completed[3].get_string_from_utf8()
	}


func _emit_result(response: Dictionary, success_signal: Signal) -> void:
	if response.status >= 200 and response.status < 300:
		var parsed = JSON.parse_string(response.body)
		if parsed is Dictionary:
			success_signal.emit(parsed)
			return
	request_failed.emit(response.status, response.body)
