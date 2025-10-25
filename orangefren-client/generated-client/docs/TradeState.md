# TradeState

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**description** | **String** | A human-readable description matching the type. | 
**r#final** | **bool** | Indicates if the state is final (no further changes expected). | 
**r#type** | **String** | The type of the trade state, determined based on the provider's status: - 'initial': Waiting for deposit. - 'confirming': Waiting for confirmations. - 'exchanging': Exchanging in progress. - 'success': Completed successfully. - 'refunded': Refunded successfully. - 'failed': Failed. - 'expired': Expired (do not send deposit; contact support if already sent). - 'unrecognized': Unrecognized status (contact support).  | 
**valid_for** | **i32** | Validity period in seconds (always 30). | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


