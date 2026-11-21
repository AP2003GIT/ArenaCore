using System;
using System.Collections;
using System.Text;
using UnityEngine;
using UnityEngine.Networking;

public sealed class ArenaCoreClient
{
    private readonly string baseUrl;

    public ArenaCoreClient(string baseUrl)
    {
        this.baseUrl = baseUrl.TrimEnd('/');
    }

    public IEnumerator CreatePlayer(
        string displayName,
        int rating,
        Action<PlayerDto> onSuccess,
        Action<string> onError)
    {
        var payload = JsonUtility.ToJson(new CreatePlayerRequest
        {
            display_name = displayName,
            rating = rating
        });

        using var request = JsonRequest(
            $"{baseUrl}/api/v1/players",
            UnityWebRequest.kHttpVerbPOST,
            payload);
        yield return request.SendWebRequest();

        if (request.result != UnityWebRequest.Result.Success)
        {
            onError(request.downloadHandler.text);
            yield break;
        }

        onSuccess(JsonUtility.FromJson<PlayerDto>(request.downloadHandler.text));
    }

    public IEnumerator Enqueue(
        string playerId,
        string mode,
        Action<MatchmakingStatusDto> onSuccess,
        Action<string> onError)
    {
        var payload = JsonUtility.ToJson(new EnqueueRequest
        {
            player_id = playerId,
            mode = mode
        });

        using var request = JsonRequest(
            $"{baseUrl}/api/v1/matchmaking/enqueue",
            UnityWebRequest.kHttpVerbPOST,
            payload);
        yield return request.SendWebRequest();

        if (request.result != UnityWebRequest.Result.Success)
        {
            onError(request.downloadHandler.text);
            yield break;
        }

        onSuccess(JsonUtility.FromJson<MatchmakingStatusDto>(request.downloadHandler.text));
    }

    private static UnityWebRequest JsonRequest(string url, string method, string payload)
    {
        var request = new UnityWebRequest(url, method);
        request.uploadHandler = new UploadHandlerRaw(Encoding.UTF8.GetBytes(payload));
        request.downloadHandler = new DownloadHandlerBuffer();
        request.SetRequestHeader("Content-Type", "application/json");
        request.timeout = 10;
        return request;
    }

    [Serializable]
    private sealed class CreatePlayerRequest
    {
        public string display_name;
        public int rating;
    }

    [Serializable]
    private sealed class EnqueueRequest
    {
        public string player_id;
        public string mode;
    }

    [Serializable]
    public sealed class PlayerDto
    {
        public string id;
        public string display_name;
        public int rating;
    }

    [Serializable]
    public sealed class MatchmakingStatusDto
    {
        public string player_id;
        public string status;
        public string mode;
        public string match_id;
    }
}
