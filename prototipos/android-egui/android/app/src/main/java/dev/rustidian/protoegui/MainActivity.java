package dev.rustidian.protoegui;

import com.google.androidgamesdk.GameActivity;

public class MainActivity extends GameActivity {
    static {
        System.loadLibrary("rustidian_proto_egui");
    }
}
