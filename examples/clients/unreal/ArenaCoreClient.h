#pragma once

#include "CoreMinimal.h"
#include "UObject/Object.h"
#include "ArenaCoreClient.generated.h"

DECLARE_DELEGATE_OneParam(FArenaCoreJsonResponse, const TSharedPtr<FJsonObject>&);
DECLARE_DELEGATE_TwoParams(FArenaCoreRequestError, int32, const FString&);

UCLASS()
class UArenaCoreClient : public UObject
{
    GENERATED_BODY()

public:
    void Initialize(FString InBaseUrl);
    void CreatePlayer(
        const FString& DisplayName,
        int32 Rating,
        FArenaCoreJsonResponse OnSuccess,
        FArenaCoreRequestError OnError);
    void Enqueue(
        const FString& PlayerId,
        const FString& Mode,
        FArenaCoreJsonResponse OnSuccess,
        FArenaCoreRequestError OnError);

private:
    FString BaseUrl;
    void PostJson(
        const FString& Path,
        const TSharedRef<FJsonObject>& Body,
        FArenaCoreJsonResponse OnSuccess,
        FArenaCoreRequestError OnError);
};
