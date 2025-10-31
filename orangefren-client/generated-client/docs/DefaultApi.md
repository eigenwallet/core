# \DefaultApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**api_eigenwallet_create_path_get**](DefaultApi.md#api_eigenwallet_create_path_get) | **GET** /api/eigenwallet_create_path | Create a new trade path (GET alias)
[**api_eigenwallet_create_path_post**](DefaultApi.md#api_eigenwallet_create_path_post) | **POST** /api/eigenwallet_create_path | Create a new trade path
[**api_eigenwallet_get_path_path_uuid_get**](DefaultApi.md#api_eigenwallet_get_path_path_uuid_get) | **GET** /api/eigenwallet_get_path/{path_uuid} | Retrieve path information by UUID
[**api_eigenwallet_get_path_path_uuid_post**](DefaultApi.md#api_eigenwallet_get_path_path_uuid_post) | **POST** /api/eigenwallet_get_path/{path_uuid} | Retrieve path information by UUID (POST alias)



## api_eigenwallet_create_path_get

> models::CreatePathResponse api_eigenwallet_create_path_get(create_path_request)
Create a new trade path (GET alias)

Creates a new trade path based on the provided parameters.  Although supports GET, it is recommended to use POST with a JSON body for parameter submission. The endpoint processes currency conversion parameters, creates a path, and returns the path UUID. If invalid parameters (e.g., priority not in ['rate', 'kyc_grade']), returns an empty object.

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**create_path_request** | [**CreatePathRequest**](CreatePathRequest.md) | JSON parameters for creating the trade path. | [required] |

### Return type

[**models::CreatePathResponse**](CreatePathResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## api_eigenwallet_create_path_post

> models::CreatePathResponse api_eigenwallet_create_path_post(create_path_request)
Create a new trade path

Creates a new trade path based on the provided JSON parameters. Processes currency conversion details, optional priorities, service inclusions/exclusions, and returns the path UUID. If invalid parameters (e.g., priority not in ['rate', 'kyc_grade']), returns an empty object.

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**create_path_request** | [**CreatePathRequest**](CreatePathRequest.md) | JSON parameters for creating the trade path. | [required] |

### Return type

[**models::CreatePathResponse**](CreatePathResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## api_eigenwallet_get_path_path_uuid_get

> models::PathResponse api_eigenwallet_get_path_path_uuid_get(path_uuid)
Retrieve path information by UUID

Fetches details about a trade path using the provided UUID.  The response includes URLs for clearnet and Tor, chat support links, the overall path state,  and optionally a list of trades if the path data is a list of trades. Supports both GET and POST methods, though no request body is required or utilized. 

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**path_uuid** | **String** | The unique identifier for the trade path. | [required] |

### Return type

[**models::PathResponse**](PathResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## api_eigenwallet_get_path_path_uuid_post

> models::PathResponse api_eigenwallet_get_path_path_uuid_post(path_uuid)
Retrieve path information by UUID (POST alias)

Identical to the GET method; fetches details about a trade path using the provided UUID. No request body is required or utilized. 

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**path_uuid** | **String** | The unique identifier for the trade path. | [required] |

### Return type

[**models::PathResponse**](PathResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

