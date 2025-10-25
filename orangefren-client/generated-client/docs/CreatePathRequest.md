# CreatePathRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**from_amount** | **f64** | The amount to send from. | 
**from_currency** | **String** | The from currency code, e.g., 'BTC' or 'BTC@BTC' (symbol@network). | 
**include_services** | Option<[**models::CreatePathRequestIncludeServices**](CreatePathRequest_include_services.md)> |  | [optional]
**priority** | Option<**String**> | Priority for path creation (rate or KYC grade). | [optional][default to Rate]
**service_variety** | Option<**String**> | Service variety filter. 'never_kyc' excludes services requiring KYC. | [optional][default to All]
**to_address** | **String** | The withdrawal address for the to_currency. | 
**to_currency** | **String** | The to currency code, e.g., 'ETH' or 'ETH@ETH' (symbol@network). | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


