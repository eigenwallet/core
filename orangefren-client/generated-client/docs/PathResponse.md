# PathResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**chat** | [**models::PathResponseChat**](PathResponse_chat.md) |  | 
**path_uuid** | **String** | The UUID of the path. | 
**state** | [**models::PathState**](PathState.md) |  | 
**trades** | Option<[**Vec<models::Trade>**](Trade.md)> | List of trades in the path. Only present if the path_data is a non-empty list of trades. | [optional]
**url** | [**models::PathResponseUrl**](PathResponse_url.md) |  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


