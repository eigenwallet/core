# PathState

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**description** | **String** | A human-readable description of the state. For 'error', this includes the specific error message. | 
**r#final** | **bool** | Indicates if the state is final (no further changes expected). | 
**r#type** | **String** | The type of the path creation state: - 'not found': Path could not be found (returned with 404 status). - 'queued': Path creation is queued and in progress. - 'error': An error occurred during path creation. - 'created': Path created successfully (for empty trade lists).  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


