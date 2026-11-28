TEST(operation, projCRS_to_projCRS_context_compatible_area_bis) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem(
            "3844"), // Pulkovo 42 Stereo 70 (Romania)
        authFactory->createCoordinateReferenceSystem("32634"), // UTM 34
        ctxt);
    ASSERT_EQ(list.size(), 3U);
    EXPECT_EQ(list[0]->nameStr(), "Inverse of Stereo 70 + "
                                  "Pulkovo 1942(58) to WGS 84 "
                                  "(19) + UTM zone 34N");
    ASSERT_EQ(list[0]->coordinateOperationAccuracies().size(), 1U);
    EXPECT_EQ(list[0]->coordinateOperationAccuracies()[0]->value(), "3");
}

