package com.carriez.flutter_hbb

import java.nio.ByteBuffer
import java.util.Timer
import java.util.TimerTask

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.util.Log

import hbb.MessageOuterClass.ClipboardFormat
import hbb.MessageOuterClass.Clipboard
import hbb.MessageOuterClass.MultiClipboards

import ffi.FFI

class RdClipboardManager(private val clipboardManager: ClipboardManager) {
    private val logTag = "RdClipboardManager"
    private val supportedMimeTypes = arrayOf(
        ClipDescription.MIMETYPE_TEXT_PLAIN,
        ClipDescription.MIMETYPE_TEXT_HTML
    )

    private var lastUpdatedClipData: ClipData? = null

    private var isServerListening = false
    private var isClientListening = false

    private val isListening: Boolean
        get() = isServerListening || isClientListening

    private val clipboardListener = object : ClipboardManager.OnPrimaryClipChangedListener {
        override fun onPrimaryClipChanged() {
            Log.d(logTag, "onPrimaryClipChanged")

            val clipData = clipboardManager.primaryClip
            if (clipData != null && clipData.itemCount > 0) {
                // Only handle the first item in the clipboard for now.
                val clip = clipData.getItemAt(0)
                if (lastUpdatedClipData != null && isClipboardDataEqual(clipData, lastUpdatedClipData!!)) {
                    Log.d(logTag, "Clipboard data is the same as last update, ignore")
                    return
                }
                val mimeTypeCount = clipData.description.getMimeTypeCount()
                val mimeTypes = mutableListOf<String>()
                for (i in 0 until mimeTypeCount) {
                    mimeTypes.add(clipData.description.getMimeType(i))
                }
                var text: CharSequence? = null;
                var html: String? = null;
                if (isSupportedMimeType(ClipDescription.MIMETYPE_TEXT_PLAIN)) {
                    text = clip?.text
                }
                if (isSupportedMimeType(ClipDescription.MIMETYPE_TEXT_HTML)) {
                    text = clip?.text
                    html = clip?.htmlText
                }
                var count = 0
                val clips = MultiClipboards.newBuilder()
                if (text != null) {
                    val content = com.google.protobuf.ByteString.copyFromUtf8(text.toString())
                        clips.addClipboards(Clipboard.newBuilder().setFormat(ClipboardFormat.Text).setContent(content).build())
                        count++
                    }
                if (html != null) {
                    val content = com.google.protobuf.ByteString.copyFromUtf8(html)
                    clips.addClipboards(Clipboard.newBuilder().setFormat(ClipboardFormat.Html).setContent(content).build())
                    count++
                }
                if (count > 0) {
                    val clipsBytes = clips.build().toByteArray()
                    val clipsBuf = ByteBuffer.allocateDirect(clipsBytes.size).put(clipsBytes)
                    clipsBuf.flip()
                    lastUpdatedClipData = clipData
                    FFI.onClipboardUpdate(clipsBuf)
                }
            }
        }
    }

    private fun isSupportedMimeType(mimeType: String): Boolean {
        return supportedMimeTypes.contains(mimeType)
    }

    private fun isClipboardDataEqual(left: ClipData, right: ClipData): Boolean {
        if (left.description.getMimeTypeCount() != right.description.getMimeTypeCount()) {
            return false
        }
        val mimeTypeCount = left.description.getMimeTypeCount()
        for (i in 0 until mimeTypeCount) {
            if (left.description.getMimeType(i) != right.description.getMimeType(i)) {
                return false
            }
        }

        if (left.itemCount != right.itemCount) {
            return false
        }
        for (i in 0 until left.itemCount) {
            val mimeType = left.description.getMimeType(i)
            if (!isSupportedMimeType(mimeType)) {
                continue
            }
            val leftItem = left.getItemAt(i)
            val rightItem = right.getItemAt(i)
            if (mimeType == ClipDescription.MIMETYPE_TEXT_PLAIN || mimeType == ClipDescription.MIMETYPE_TEXT_HTML) {
                if (leftItem.text != rightItem.text || leftItem.htmlText != rightItem.htmlText) {
                    return false
                }
            }
        }
        return true
    }

    // enable:
    // disable all - 0, enable all - 1
    // client_disable - 2, client_enable - 3
    // server_disable - 4, server_enable - 5
    @Keep
    fun rustenableClipboard(enable: Int) {
        Log.d(logTag, "enableClipboard: enable: $enable, isServerListening: $isServerListening, isClientListening: $isClientListening")
        if (enable == 0) {
            if (isListening) {
                clipboardManager.removePrimaryClipChangedListener(clipboardListener)
            }
            isServerListening = false
            isClientListening = false
        } else if (enable == 1) {
            if (!isListening) {
                clipboardManager.addPrimaryClipChangedListener(clipboardListener)
            }
            isServerListening = true
            isClientListening = true
        } else if (enable == 2) {
            if (isClientListening && !isServerListening) {
                clipboardManager.removePrimaryClipChangedListener(clipboardListener)
            }
            isClientListening = false
        } else if (enable == 3) {
            if (!isClientListening && !isServerListening) {
                clipboardManager.addPrimaryClipChangedListener(clipboardListener)
            }
            isClientListening = true
        } else if (enable == 4) {
            if (isServerListening && !isClientListening) {
                clipboardManager.removePrimaryClipChangedListener(clipboardListener)
            }
            isServerListening = false
        } else if (enable == 5) {
            if (!isServerListening && !isClientListening) {
                clipboardManager.addPrimaryClipChangedListener(clipboardListener)
            }
            isServerListening = true
        }
        if (!isListening) {
            lastUpdatedClipData = null
        }
    }

    fun syncClipboard(checkListening: Boolean) {
        if (checkListening && !isListening) {
            return
        }
        clipboardListener.onPrimaryClipChanged()
    }

    @Keep
    fun rustUpdateClipboard(clips: ByteArray) {
        val clips = MultiClipboards.parseFrom(clips)
        var mimeTypes = mutableListOf<String>()
        var text: String? = null
        var html: String? = null
        for (clip in clips.getClipboardsList()) {
            when (clip.format) {
                    ClipboardFormat.Text -> {
                        mimeTypes.add(ClipDescription.MIMETYPE_TEXT_PLAIN)
                    text = String(clip.content.toByteArray(), Charsets.UTF_8)
                }
                ClipboardFormat.Html -> {
                    mimeTypes.add(ClipDescription.MIMETYPE_TEXT_HTML)
                    html = String(clip.content.toByteArray(), Charsets.UTF_8)
                }
                ClipboardFormat.ImageRgba -> {
                }
                ClipboardFormat.ImagePng -> {
                }
                else -> {
                    Log.e(logTag, "Unsupported clipboard format: ${clip.format}")
                }
            }
        }

        val clipDescription = ClipDescription("clipboard", mimeTypes.toTypedArray())
        var item: ClipData.Item? = null
        if (text == null) {
            Log.e(logTag, "No text content in clipboard")
            return
        } else {
            if (html == null) {
                item = ClipData.Item(text)
            } else {
                item = ClipData.Item(text, html)
            }
        }
        if (item == null) {
            Log.e(logTag, "No item in clipboard")
            return
        }
        val clipData = ClipData(clipDescription, item)
        lastUpdatedClipData = clipData
        clipboardManager.setPrimaryClip(clipData)
    }
}
