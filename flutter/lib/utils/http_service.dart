import 'dart:convert';
import 'package:http/http.dart' as http;
import '../models/platform_model.dart';
export 'package:http/http.dart' show Response;

enum HttpMethod { get, post, put, delete }

class HttpService {
  Future<http.Response> sendRequest(
    Uri url,
    HttpMethod method, {
    Map<String, String>? headers,
    dynamic body,
  }) async {
    headers ??= {'Content-Type': 'application/json'};

    // Determine if there is currently a proxy setting, and if so, use FFI to call the Rust HTTP method.
    final isProxy = await bind.mainGetProxyStatus();

    if (!isProxy) {
      return await _pollFultterHttp(url, method, headers: headers, body: body);
    }

    String headersJson = jsonEncode(headers);
    String methodName = method.toString().split('.').last;
    await bind.mainHttpRequest(
        url: url.toString(),
        method: methodName.toLowerCase(),
        body: body,
        header: headersJson);

    var resJson = await _pollForResponse(url.toString());
    return _parseHttpResponse(resJson);
  }

  Future<http.Response> _pollFultterHttp(
    Uri url,
    HttpMethod method, {
    Map<String, String>? headers,
    dynamic body,
  }) async {
    var response = http.Response('', 400);

    switch (method) {
      case HttpMethod.get:
        // Note: `GET` or `HEAD` always follows redirects, though the `followRedirects` is set to `false`.
        // final request = http.Request('GET', url)..followRedirects = false;
        // response = await request.send().then(http.Response.fromStream);
        response = await http.get(url, headers: headers);
        break;
      case HttpMethod.post:
        response = await http.post(url, headers: headers, body: body);
        break;
      case HttpMethod.put:
        response = await http.put(url, headers: headers, body: body);
        break;
      case HttpMethod.delete:
        response = await http.delete(url, headers: headers, body: body);
        break;
      default:
        throw Exception('Unsupported HTTP method');
    }
    return response;
  }

  Future<String> _pollForResponse(String url) async {
    String? responseJson = " ";
    while (responseJson == " ") {
      responseJson = await bind.mainGetHttpStatus(url: url);
      if (responseJson == null) {
        throw Exception('The HTTP request failed');
      }
      if (responseJson == " ") {
        await Future.delayed(const Duration(milliseconds: 100));
      }
    }
    return responseJson!;
  }

  http.Response _parseHttpResponse(String responseJson) {
    try {
      var parsedJson = jsonDecode(responseJson);
      String body = parsedJson['body'];
      Map<String, String> headers = {};
      for (var key in parsedJson['headers'].keys) {
        headers[key] = parsedJson['headers'][key];
      }
      int statusCode = parsedJson['status_code'];
      return http.Response(body, statusCode, headers: headers);
    } catch (e) {
      throw Exception('Failed to parse response: $e');
    }
  }
}

const int _kMaxRedirects = 5;
// Manually handle redirects for the Flutter HTTP client.
// dart:io's HttpClient follows redirects automatically, but there's a bug.
// The follwing code takes no effect if redirects are already followed.
//
// https://github.com/dart-lang/sdk/issues/54001
// The requests other than `GET` or `HEAD` don't follow redirects for status code 307 and 308.
Future<http.Response> _handleRedirect(
    Uri url, Future<http.Response> Function(Uri url) doRequest) async {
  for (int i = 0; i < _kMaxRedirects; i++) {
    final response = await doRequest(url);
    // Don't use `response.isRedirect` here, it's false while the status code is 307 and 308.
    // https://github.com/dart-lang/sdk/issues/49210
    // https://github.com/dart-lang/sdk/issues/54001
    if (response.statusCode >= 300 && response.statusCode < 400) {
      final location = response.headers['location'];
      if (location == null) {
        throw Exception('Redirect response missing location header');
      } else {
        url = Uri.parse(location);
      }
    } else {
      return response;
    }
  }
  throw Exception('Too many redirects');
}

Future<http.Response> get(Uri url, {Map<String, String>? headers}) async {
  final httpService = HttpService();
  return await _handleRedirect(url, (url) async {
    return await httpService.sendRequest(url, HttpMethod.get, headers: headers);
  });
}

Future<http.Response> post(Uri url,
    {Map<String, String>? headers, Object? body, Encoding? encoding}) async {
  final httpService = HttpService();
  return await _handleRedirect(url, (url) async {
    return await httpService.sendRequest(url, HttpMethod.post,
        body: body, headers: headers);
  });
}

Future<http.Response> put(Uri url,
    {Map<String, String>? headers, Object? body, Encoding? encoding}) async {
  final httpService = HttpService();
  return await _handleRedirect(url, (url) async {
    return await httpService.sendRequest(url, HttpMethod.put,
        body: body, headers: headers);
  });
}

Future<http.Response> delete(Uri url,
    {Map<String, String>? headers, Object? body, Encoding? encoding}) async {
  final httpService = HttpService();
  return await _handleRedirect(url, (url) async {
    return await httpService.sendRequest(url, HttpMethod.delete,
        body: body, headers: headers);
  });
}
