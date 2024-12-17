import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:get/get.dart';
import 'package:auto_size_text/auto_size_text.dart';

void main() {
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  // This widget is the root of your application.
  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Flutter Demo',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple),
        useMaterial3: true,
      ),
      home: const MyHomePage(title: 'Flutter Demo Home Page'),
    );
  }
}

class IDTextEditingController extends TextEditingController {
  IDTextEditingController({String? text}) : super(text: text);

  String get id => trimID(value.text);

  set id(String newID) => text = formatID(newID);
}

class IDTextInputFormatter extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(
      TextEditingValue oldValue, TextEditingValue newValue) {
    if (newValue.text.isEmpty) {
      return newValue.copyWith(text: '');
    } else if (newValue.text.compareTo(oldValue.text) == 0) {
      return newValue;
    } else {
      int selectionIndexFromTheRight =
          newValue.text.length - newValue.selection.extentOffset;
      String newID = formatID(newValue.text);
      return TextEditingValue(
        text: newID,
        selection: TextSelection.collapsed(
          offset: newID.length - selectionIndexFromTheRight,
        ),
        // https://github.com/flutter/flutter/issues/78066#issuecomment-797869906
        composing: newValue.composing,
      );
    }
  }
}

String formatID(String id) {
  String id2 = id.replaceAll(' ', '');
  String suffix = '';
  if (id2.endsWith(r'\r') || id2.endsWith(r'/r')) {
    suffix = id2.substring(id2.length - 2, id2.length);
    id2 = id2.substring(0, id2.length - 2);
  }
  if (int.tryParse(id2) == null) return id;
  String newID = '';
  if (id2.length <= 3) {
    newID = id2;
  } else {
    var n = id2.length;
    var a = n % 3 != 0 ? n % 3 : 3;
    newID = id2.substring(0, a);
    for (var i = a; i < n; i += 3) {
      newID += " ${id2.substring(i, i + 3)}";
    }
  }
  return newID + suffix;
}

String trimID(String id) {
  return id.replaceAll(' ', '');
}

class MyHomePage extends StatefulWidget {
  const MyHomePage({super.key, required this.title});

  final String title;

  @override
  State<MyHomePage> createState() => _MyHomePageState();
}

class _MyHomePageState extends State<MyHomePage> {
  final RxBool _idInputFocused = false.obs;
  final _idController = IDTextEditingController();

  Widget getConnectionPageTitle(BuildContext context, bool isWeb) {
  return Row(
    children: [
      Expanded(
          child: Row(
        children: [
          AutoSizeText(
            'Control Remote Desktop',
            maxLines: 1,
            style: Theme.of(context)
                .textTheme
                .titleLarge
                ?.merge(TextStyle(height: 1)),
          ).marginOnly(right: 4),
          Tooltip(
            waitDuration: Duration(milliseconds: 300),
            message: "id_input_tip",
            child: Icon(
              Icons.help_outline_outlined,
              size: 16,
              color: Theme.of(context)
                  .textTheme
                  .titleLarge
                  ?.color
                  ?.withOpacity(0.5),
            ),
          ),
        ],
      )),
    ],
  );
}

  Widget _buildRemoteIDTextField(BuildContext context) {
    var w = Container(
      width: 320 + 20 * 2,
      padding: const EdgeInsets.fromLTRB(20, 24, 20, 22),
      decoration: BoxDecoration(
          borderRadius: const BorderRadius.all(Radius.circular(13)),
          border: Border.all(color: Theme.of(context).colorScheme.background)),
      child: Ink(
        child: Column(
          children: [
            getConnectionPageTitle(context, false).marginOnly(bottom: 15),
            Row(
              children: [
                Expanded(
                    child: TextField(
                  autocorrect: false,
                  enableSuggestions: false,
                  keyboardType: TextInputType.visiblePassword,
                  style: const TextStyle(
                    fontFamily: 'WorkSans',
                    fontSize: 22,
                    height: 1.4,
                  ),
                  maxLines: 1,
                  cursorColor: Theme.of(context).textTheme.titleLarge?.color,
                  decoration: InputDecoration(
                      filled: false,
                      counterText: '',
                      hintText: _idInputFocused.value
                          ? null
                          : 'Enter Remote ID',
                      contentPadding: const EdgeInsets.symmetric(
                          horizontal: 15, vertical: 13)),
                  inputFormatters: [IDTextInputFormatter()],
                  onChanged: (v) {
                    _idController.id = v;
                  },
                  onSubmitted: (_) {
                    print('onSubmitted');
                  },
                )),
              ],
            ),
            Padding(
              padding: const EdgeInsets.only(top: 13.0),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  const SizedBox(
                    width: 17,
                  ),
                ],
              ),
            )
          ],
        ),
      ),
    );
    return Container(
        constraints: const BoxConstraints(maxWidth: 600), child: w);
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        backgroundColor: Theme.of(context).colorScheme.inversePrimary,
        title: Text(widget.title),
      ),
      body: Center(
        child: _buildRemoteIDTextField(context),
      ),
    );
  }
}
