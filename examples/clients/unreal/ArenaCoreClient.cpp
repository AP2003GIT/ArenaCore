#include "ArenaCoreClient.h"

#include "Dom/JsonObject.h"
#include "HttpModule.h"
#include "Interfaces/IHttpResponse.h"
#include "Serialization/JsonSerializer.h"
#include "Serialization/JsonWriter.h"

void UArenaCoreClient::Initialize(FString InBaseUrl)
{
    BaseUrl = MoveTemp(InBaseUrl);
    BaseUrl.RemoveFromEnd(TEXT("/"));
}

void UArenaCoreClient::CreatePlayer(
    const FString& DisplayName,
    int32 Rating,
    FArenaCoreJsonResponse OnSuccess,
    FArenaCoreRequestError OnError)
{
    const TSharedRef<FJsonObject> Body = MakeShared<FJsonObject>();
    Body->SetStringField(TEXT("display_name"), DisplayName);
    Body->SetNumberField(TEXT("rating"), Rating);
    PostJson(TEXT("/api/v1/players"), Body, MoveTemp(OnSuccess), MoveTemp(OnError));
}

void UArenaCoreClient::Enqueue(
    const FString& PlayerId,
    const FString& Mode,
    FArenaCoreJsonResponse OnSuccess,
    FArenaCoreRequestError OnError)
{
    const TSharedRef<FJsonObject> Body = MakeShared<FJsonObject>();
    Body->SetStringField(TEXT("player_id"), PlayerId);
    Body->SetStringField(TEXT("mode"), Mode);
    PostJson(TEXT("/api/v1/matchmaking/enqueue"), Body, MoveTemp(OnSuccess), MoveTemp(OnError));
}

void UArenaCoreClient::PostJson(
    const FString& Path,
    const TSharedRef<FJsonObject>& Body,
    FArenaCoreJsonResponse OnSuccess,
    FArenaCoreRequestError OnError)
{
    FString Payload;
    const TSharedRef<TJsonWriter<>> Writer = TJsonWriterFactory<>::Create(&Payload);
    FJsonSerializer::Serialize(Body, Writer);

    const TSharedRef<IHttpRequest, ESPMode::ThreadSafe> Request = FHttpModule::Get().CreateRequest();
    Request->SetURL(BaseUrl + Path);
    Request->SetVerb(TEXT("POST"));
    Request->SetHeader(TEXT("Content-Type"), TEXT("application/json"));
    Request->SetContentAsString(Payload);
    Request->SetTimeout(10.0f);
    Request->OnProcessRequestComplete().BindLambda(
        [OnSuccess = MoveTemp(OnSuccess), OnError = MoveTemp(OnError)](
            FHttpRequestPtr,
            FHttpResponsePtr Response,
            bool bConnectedSuccessfully)
        {
            if (!bConnectedSuccessfully || !Response.IsValid())
            {
                OnError.ExecuteIfBound(0, TEXT("ArenaCore connection failed"));
                return;
            }

            const int32 Status = Response->GetResponseCode();
            if (Status < 200 || Status >= 300)
            {
                OnError.ExecuteIfBound(Status, Response->GetContentAsString());
                return;
            }

            TSharedPtr<FJsonObject> Json;
            const TSharedRef<TJsonReader<>> Reader =
                TJsonReaderFactory<>::Create(Response->GetContentAsString());
            if (!FJsonSerializer::Deserialize(Reader, Json) || !Json.IsValid())
            {
                OnError.ExecuteIfBound(Status, TEXT("ArenaCore returned invalid JSON"));
                return;
            }

            OnSuccess.ExecuteIfBound(Json);
        });
    Request->ProcessRequest();
}
